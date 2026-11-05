// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! UI events (DESIGN D6). [`Pointer`] turns pointer input into DOM-style events and
//! keeps `:hover` / `:active` up to date; [`crate::Interaction`] adds focus and keys
//! and runs default actions. Dispatching to scripts is the caller's job:
//! [`UiEvent::script_args`] are the arguments for `__platypus_ui_event`, which
//! `@platypus/react` runs through capture and bubble phases.

use web_ui_core::{NodeId, Value};

use crate::attrs::ElementState;
use crate::tree::UiTree;

/// Two presses closer than this (in time and distance) count as a double click.
pub const DOUBLE_CLICK_MS: f64 = 500.0;
pub const DOUBLE_CLICK_DISTANCE: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    PointerMove,
    PointerDown,
    PointerUp,
    PointerEnter,
    PointerLeave,
    Click,
    DblClick,
    ContextMenu,
    Wheel,
    KeyDown,
    KeyUp,
    /// Focus arrived (bubbles; React's `onFocus`).
    FocusIn,
    /// Focus left (bubbles; React's `onBlur`).
    FocusOut,
    /// An element scrolled (doesn't bubble).
    Scroll,
    /// A text field's text changed (bubbles; React's `onChange` / `onInput`).
    Input,
    /// A modal dialog was asked to close (Escape, a backdrop click); cancelable,
    /// doesn't bubble.
    Cancel,
    /// A dialog should close: the `cancel` before it wasn't prevented. Scripts close
    /// it by clearing `open`. Doesn't bubble.
    Close,
    /// Transitions (bubble; `value` is the kebab-case property, `delta_x` the elapsed
    /// seconds): created (`run`), past its delay (`start`), done, or replaced/removed.
    TransitionRun,
    TransitionStart,
    TransitionEnd,
    TransitionCancel,
    /// Animations (bubble; `value` is the animation name, `delta_x` the elapsed seconds).
    AnimationStart,
    AnimationIteration,
    AnimationEnd,
    AnimationCancel,
    /// The element took (`got`) or lost pointer capture (bubble).
    GotPointerCapture,
    LostPointerCapture,
    /// Drag and drop (all bubble). `dragstart`, `drag` and `dragend` go to the dragged
    /// element; `dragenter`, `dragover`, `dragleave` and `drop` to the element under
    /// the pointer. Preventing `dragstart` cancels the drag; preventing `dragover`
    /// accepts a drop there. `dragend`'s `detail` is 1 when it was dropped. Files
    /// dragged in from the system come as `files` (no `dragstart` / `drag` / `dragend`).
    DragStart,
    Drag,
    DragEnter,
    DragOver,
    DragLeave,
    Drop,
    DragEnd,
}

impl EventKind {
    /// Every event type (a test keeps the list complete).
    pub const ALL: &'static [EventKind] = &[
        EventKind::PointerMove,
        EventKind::PointerDown,
        EventKind::PointerUp,
        EventKind::PointerEnter,
        EventKind::PointerLeave,
        EventKind::Click,
        EventKind::DblClick,
        EventKind::ContextMenu,
        EventKind::Wheel,
        EventKind::KeyDown,
        EventKind::KeyUp,
        EventKind::FocusIn,
        EventKind::FocusOut,
        EventKind::Scroll,
        EventKind::Input,
        EventKind::Cancel,
        EventKind::Close,
        EventKind::TransitionRun,
        EventKind::TransitionStart,
        EventKind::TransitionEnd,
        EventKind::TransitionCancel,
        EventKind::AnimationStart,
        EventKind::AnimationIteration,
        EventKind::AnimationEnd,
        EventKind::AnimationCancel,
        EventKind::GotPointerCapture,
        EventKind::LostPointerCapture,
        EventKind::DragStart,
        EventKind::Drag,
        EventKind::DragEnter,
        EventKind::DragOver,
        EventKind::DragLeave,
        EventKind::Drop,
        EventKind::DragEnd,
    ];

    /// The DOM event type.
    pub fn name(self) -> &'static str {
        match self {
            EventKind::PointerMove => "pointermove",
            EventKind::PointerDown => "pointerdown",
            EventKind::PointerUp => "pointerup",
            EventKind::PointerEnter => "pointerenter",
            EventKind::PointerLeave => "pointerleave",
            EventKind::Click => "click",
            EventKind::DblClick => "dblclick",
            EventKind::ContextMenu => "contextmenu",
            EventKind::Wheel => "wheel",
            EventKind::KeyDown => "keydown",
            EventKind::KeyUp => "keyup",
            EventKind::FocusIn => "focusin",
            EventKind::FocusOut => "focusout",
            EventKind::Scroll => "scroll",
            EventKind::Input => "input",
            EventKind::Cancel => "cancel",
            EventKind::Close => "close",
            EventKind::TransitionRun => "transitionrun",
            EventKind::TransitionStart => "transitionstart",
            EventKind::TransitionEnd => "transitionend",
            EventKind::TransitionCancel => "transitioncancel",
            EventKind::AnimationStart => "animationstart",
            EventKind::AnimationIteration => "animationiteration",
            EventKind::AnimationEnd => "animationend",
            EventKind::AnimationCancel => "animationcancel",
            EventKind::GotPointerCapture => "gotpointercapture",
            EventKind::LostPointerCapture => "lostpointercapture",
            EventKind::DragStart => "dragstart",
            EventKind::Drag => "drag",
            EventKind::DragEnter => "dragenter",
            EventKind::DragOver => "dragover",
            EventKind::DragLeave => "dragleave",
            EventKind::Drop => "drop",
            EventKind::DragEnd => "dragend",
        }
    }

    /// Drag and drop events, which carry the dragged files.
    pub fn is_drag(self) -> bool {
        matches!(
            self,
            EventKind::DragStart
                | EventKind::Drag
                | EventKind::DragEnter
                | EventKind::DragOver
                | EventKind::DragLeave
                | EventKind::Drop
                | EventKind::DragEnd
        )
    }
}

/// Held modifier keys, sent with every event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    /// shift = 1, ctrl = 2, alt = 4, meta = 8 (decoded by `@platypus/react`).
    pub fn bits(self) -> u32 {
        self.shift as u32
            | (self.ctrl as u32) << 1
            | (self.alt as u32) << 2
            | (self.meta as u32) << 3
    }
}

/// An event for scripts. Fields follow the DOM's `PointerEvent`, `WheelEvent`,
/// `KeyboardEvent` and `FocusEvent`; fields an event type doesn't use are zero/empty.
#[derive(Clone, Debug, PartialEq)]
pub struct UiEvent {
    pub kind: EventKind,
    /// The target first, then its ancestors up to (not including) the root. `pointerenter`
    /// and `pointerleave` don't bubble, so their path is just the target.
    pub path: Vec<NodeId>,
    /// Pointer position in logical pixels.
    pub x: f32,
    pub y: f32,
    /// The button that changed: 0 primary, 1 middle, 2 secondary, 3 back, 4 forward.
    pub button: u8,
    /// Held buttons as a bitmask: 1 primary, 2 secondary, 4 middle, 8 back, 16 forward.
    pub buttons: u8,
    /// The click count for presses, releases and clicks.
    pub detail: u32,
    /// Wheel deltas in logical pixels; positive `y` scrolls down.
    pub delta_x: f32,
    pub delta_y: f32,
    pub modifiers: Modifiers,
    /// Keyboard events: the DOM `key` (`"a"`, `"Enter"`) and `code` (`"KeyA"`).
    pub key: String,
    pub code: String,
    pub repeat: bool,
    /// Keyboard events in a text field: the selection as character offsets (`selectionStart`, `selectionEnd`).
    pub selection: (usize, usize),
    /// Focus events: the element losing (for `focusin`) or gaining (for `focusout`) focus.
    pub related: Option<NodeId>,
    /// Scroll events: the target's scroll position and sizes.
    pub scroll: Option<ScrollInfo>,
    /// Input events: the field's new text.
    pub value: Option<String>,
    /// Drag events: paths of files dragged in from the system (empty for drags of
    /// elements).
    pub files: Vec<String>,
}

/// A scroll container's position and sizes, like the DOM's `scrollTop` and friends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollInfo {
    pub left: f32,
    pub top: f32,
    /// Content size (`scrollWidth` / `scrollHeight`).
    pub width: f32,
    pub height: f32,
    /// Padding box size (`clientWidth` / `clientHeight`).
    pub client_width: f32,
    pub client_height: f32,
}

impl ScrollInfo {
    pub fn of(tree: &UiTree, id: NodeId) -> Self {
        let (left, top) = tree.scroll_offset(id);
        let (max_x, max_y) = tree.scroll_max(id);
        let client = tree.padding_box(id).unwrap_or_default();
        Self {
            left,
            top,
            width: client.width + max_x,
            height: client.height + max_y,
            client_width: client.width,
            client_height: client.height,
        }
    }
}

impl UiEvent {
    pub fn new(kind: EventKind, path: Vec<NodeId>, modifiers: Modifiers) -> Self {
        Self {
            kind,
            path,
            x: 0.0,
            y: 0.0,
            button: 0,
            buttons: 0,
            detail: 0,
            delta_x: 0.0,
            delta_y: 0.0,
            modifiers,
            key: String::new(),
            code: String::new(),
            repeat: false,
            selection: (0, 0),
            related: None,
            scroll: None,
            value: None,
            files: Vec::new(),
        }
    }

    pub fn target(&self) -> NodeId {
        self.path[0]
    }

    /// Arguments for `__platypus_ui_event(type, path, data, keys)`: `data` is `[x, y, button,
    /// buttons, detail, deltaX, deltaY, modifiers, repeat, related, scrollLeft, scrollTop,
    /// scrollWidth, scrollHeight, clientWidth, clientHeight]` (`related` 0 for none) and
    /// `strings` is `[key, code]` for keyboard events, `[value]` for input events and
    /// the file paths for drag events.
    pub fn script_args(&self) -> Vec<Value> {
        let path = self
            .path
            .iter()
            .map(|id| Value::Number(id.0.into()))
            .collect();
        let data = [
            self.x as f64,
            self.y as f64,
            self.button as f64,
            self.buttons as f64,
            self.detail as f64,
            self.delta_x as f64,
            self.delta_y as f64,
            self.modifiers.bits() as f64,
            self.repeat as u8 as f64,
            self.related.map_or(0.0, |id| id.0 as f64),
        ];
        let s = self.scroll.unwrap_or_default();
        let scroll = [
            s.left,
            s.top,
            s.width,
            s.height,
            s.client_width,
            s.client_height,
        ];
        let selection = [self.selection.0 as f64, self.selection.1 as f64];
        let data = data
            .into_iter()
            .chain(scroll.map(f64::from))
            .chain(selection);
        let mut args = vec![
            self.kind.name().into(),
            Value::Array(path),
            Value::Array(data.map(Value::Number).collect()),
        ];
        if matches!(self.kind, EventKind::KeyDown | EventKind::KeyUp) {
            args.push(Value::Array(vec![
                self.key.as_str().into(),
                self.code.as_str().into(),
            ]));
        }
        if let Some(value) = &self.value {
            args.push(Value::Array(vec![value.as_str().into()]));
        }
        if self.kind.is_drag() {
            args.push(Value::Array(
                self.files.iter().map(|f| f.as_str().into()).collect(),
            ));
        }
        args
    }
}

/// The DOM's `buttons` bit for a `button` number.
fn button_bit(button: u8) -> u8 {
    match button {
        0 => 1,
        1 => 4,
        2 => 2,
        b if b < 8 => 1 << b,
        _ => 0,
    }
}

#[derive(Clone, Copy, Debug)]
struct LastPress {
    target: NodeId,
    button: u8,
    time_ms: f64,
    x: f32,
    y: f32,
    count: u32,
}

/// The mouse pointer's state over a [`UiTree`]: what it hovers, which buttons are
/// held and where they were pressed.
///
/// Each method takes the tree, updates `:hover` / `:active` on it and returns the
/// events to dispatch, in order.
#[derive(Debug, Default)]
pub struct Pointer {
    pos: Option<(f32, f32)>,
    /// The hovered element's event path (target first).
    hovered: Vec<NodeId>,
    buttons: u8,
    /// Where each button (by number) was pressed.
    pressed: [Option<NodeId>; 5],
    /// The primary button's press path, which has `:active`.
    active: Vec<NodeId>,
    last_press: Option<LastPress>,
    clicks: u32,
    modifiers: Modifiers,
    /// The element capturing the pointer (`setPointerCapture`): it's the target of
    /// moves and the release wherever the pointer is, until a release ends it.
    capture: Option<NodeId>,
}

impl Pointer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// The pointer position, or `None` when it's outside the window.
    pub fn position(&self) -> Option<(f32, f32)> {
        self.pos
    }

    /// Held buttons, as the DOM's `buttons` bitmask.
    pub fn buttons(&self) -> u8 {
        self.buttons
    }

    /// The element capturing the pointer, if any.
    pub fn capture(&self) -> Option<NodeId> {
        self.capture
    }

    /// `setPointerCapture`: `id` gets the pointer's events until the buttons are
    /// released. Like browsers, only while a button is held (otherwise nothing
    /// happens); another element's capture moves to `id`.
    pub fn set_capture(&mut self, tree: &mut UiTree, id: NodeId) -> Vec<UiEvent> {
        if self.buttons == 0 || !tree.contains(id) || self.capture == Some(id) {
            return Vec::new();
        }
        let mut events = self.release(tree);
        self.capture = Some(id);
        events.push(self.event(EventKind::GotPointerCapture, tree.event_path(id), 0));
        events.extend(self.rehover(tree));
        events
    }

    /// `releasePointerCapture`: ends `id`'s capture, if it has it.
    pub fn release_capture(&mut self, tree: &mut UiTree, id: NodeId) -> Vec<UiEvent> {
        if self.capture != Some(id) {
            return Vec::new();
        }
        let mut events = self.release(tree);
        events.extend(self.rehover(tree));
        events
    }

    /// Ends any capture, with `lostpointercapture` if the element still exists.
    fn release(&mut self, tree: &UiTree) -> Vec<UiEvent> {
        match self.capture.take() {
            Some(id) if tree.contains(id) => {
                vec![self.event(EventKind::LostPointerCapture, tree.event_path(id), 0)]
            }
            _ => Vec::new(),
        }
    }

    /// The element under the pointer.
    pub fn hovered(&self) -> Option<NodeId> {
        self.hovered.first().copied()
    }

    /// The CSS cursor for the hovered element.
    pub fn cursor(&self, tree: &UiTree) -> &'static str {
        tree.cursor(self.hovered())
    }

    /// The hovered element's cursor with its custom images (see
    /// [`UiTree::cursor_with_images`]).
    pub fn cursor_with_images<'t>(
        &self,
        tree: &'t UiTree,
    ) -> (&'static str, &'t [crate::style::CursorImage]) {
        tree.cursor_with_images(self.hovered())
    }

    /// The pointer moved to (x, y), in logical pixels.
    pub fn move_to(&mut self, tree: &mut UiTree, x: f32, y: f32) -> Vec<UiEvent> {
        self.pos = Some((x, y));
        let mut events = self.rehover(tree);
        if !self.hovered.is_empty() {
            events.push(self.event(EventKind::PointerMove, self.hovered.clone(), 0));
        }
        events
    }

    /// The pointer left the window.
    pub fn leave(&mut self, tree: &mut UiTree) -> Vec<UiEvent> {
        self.pos = None;
        self.rehover(tree)
    }

    /// Re-hit-tests at the current position, for when the tree changed under a still
    /// pointer (an element moved in or out). Call once per frame after commits.
    pub fn refresh(&mut self, tree: &mut UiTree) -> Vec<UiEvent> {
        self.active.retain(|id| tree.contains(*id));
        // A removed (or hidden) element loses its capture.
        if self.capture.is_some_and(|id| !tree.is_displayed(id)) {
            self.capture = None;
        }
        self.rehover(tree)
    }

    pub fn down(&mut self, tree: &mut UiTree, button: u8, time_ms: f64) -> Vec<UiEvent> {
        let mut events = self.rehover(tree);
        self.buttons |= button_bit(button);
        let Some(target) = self.hovered() else {
            return events;
        };
        let (x, y) = self.pos.unwrap_or_default();
        self.clicks = match self.last_press {
            Some(last)
                if last.target == target
                    && last.button == button
                    && time_ms - last.time_ms <= DOUBLE_CLICK_MS
                    && (last.x - x).hypot(last.y - y) <= DOUBLE_CLICK_DISTANCE =>
            {
                last.count + 1
            }
            _ => 1,
        };
        self.last_press = Some(LastPress {
            target,
            button,
            time_ms,
            x,
            y,
            count: self.clicks,
        });
        if let Some(slot) = self.pressed.get_mut(button as usize) {
            *slot = Some(target);
        }
        if button == 0 {
            self.set_active(tree, self.hovered.clone());
        }
        events.push(self.event(EventKind::PointerDown, self.hovered.clone(), button));
        events
    }

    pub fn up(&mut self, tree: &mut UiTree, button: u8) -> Vec<UiEvent> {
        let mut events = self.rehover(tree);
        self.buttons &= !button_bit(button);
        if button == 0 {
            self.set_active(tree, Vec::new());
        }
        let pressed = self.pressed.get_mut(button as usize).and_then(Option::take);
        if self.hovered.is_empty() {
            return events;
        }
        events.push(self.event(EventKind::PointerUp, self.hovered.clone(), button));

        // A click goes to the nearest element containing both the press and the release.
        let common = pressed.and_then(|down| {
            let down_path = tree.event_path(down);
            self.hovered.iter().position(|id| down_path.contains(id))
        });
        if let Some(i) = common {
            let path = self.hovered[i..].to_vec();
            match button {
                0 => {
                    events.push(self.event(EventKind::Click, path.clone(), button));
                    if self.clicks == 2 {
                        events.push(self.event(EventKind::DblClick, path, button));
                    }
                }
                2 => events.push(self.event(EventKind::ContextMenu, path, button)),
                _ => {}
            }
        }
        // Releasing the last button ends a capture; hover goes back to what's under
        // the pointer.
        if self.buttons == 0 && self.capture.is_some() {
            events.extend(self.release(tree));
            events.extend(self.rehover(tree));
        }
        events
    }

    /// Wheel deltas in logical pixels, positive `dy` scrolling down.
    pub fn wheel(&mut self, tree: &mut UiTree, dx: f32, dy: f32) -> Vec<UiEvent> {
        let mut events = self.rehover(tree);
        if !self.hovered.is_empty() {
            let mut e = self.event(EventKind::Wheel, self.hovered.clone(), 0);
            e.delta_x = dx;
            e.delta_y = dy;
            events.push(e);
        }
        events
    }

    /// Moves the pointer without hit-testing (during a drag, hover stays put).
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.pos = Some((x, y));
    }

    /// The primary press became a drag: no click will follow it, `:active` and any
    /// capture end (with `lostpointercapture`). Buttons stay held.
    pub fn cancel_press(&mut self, tree: &mut UiTree) -> Vec<UiEvent> {
        self.pressed[0] = None;
        self.set_active(tree, Vec::new());
        self.release(tree)
    }

    /// Forgets held buttons (the window lost focus, so releases may never arrive).
    pub fn cancel(&mut self, tree: &mut UiTree) {
        self.buttons = 0;
        self.pressed = [None; 5];
        self.capture = None;
        self.set_active(tree, Vec::new());
    }

    fn event(&self, kind: EventKind, path: Vec<NodeId>, button: u8) -> UiEvent {
        let (x, y) = self.pos.unwrap_or_default();
        let detail = match kind {
            EventKind::PointerDown | EventKind::PointerUp | EventKind::Click => self.clicks,
            EventKind::DblClick => 2,
            _ => 0,
        };
        UiEvent {
            x,
            y,
            button,
            buttons: self.buttons,
            detail,
            ..UiEvent::new(kind, path, self.modifiers)
        }
    }

    /// Hit-tests at the current position and moves `:hover`, returning
    /// `pointerleave`s (innermost first) then `pointerenter`s (outermost first).
    fn rehover(&mut self, tree: &mut UiTree) -> Vec<UiEvent> {
        // A capturing element is the target wherever the pointer is.
        let target = match self.capture.filter(|id| tree.contains(*id)) {
            Some(id) => Some(id),
            None => self.pos.and_then(|(x, y)| tree.hit_test(x, y)),
        };
        let path = target.map(|t| tree.event_path(t)).unwrap_or_default();
        if path == self.hovered {
            return Vec::new();
        }
        let old = std::mem::replace(&mut self.hovered, path);
        let mut events = Vec::new();
        for id in old.iter().filter(|id| !self.hovered.contains(id)) {
            if tree.contains(*id) {
                update_state(tree, *id, |s| s.hover = false);
                events.push(self.event(EventKind::PointerLeave, vec![*id], 0));
            }
        }
        for id in self.hovered.iter().rev().filter(|id| !old.contains(id)) {
            update_state(tree, *id, |s| s.hover = true);
            events.push(self.event(EventKind::PointerEnter, vec![*id], 0));
        }
        events
    }

    fn set_active(&mut self, tree: &mut UiTree, path: Vec<NodeId>) {
        for id in self.active.iter().filter(|id| !path.contains(id)) {
            update_state(tree, *id, |s| s.active = false);
        }
        for id in &path {
            update_state(tree, *id, |s| s.active = true);
        }
        self.active = path;
    }
}

pub(crate) fn update_state(tree: &mut UiTree, id: NodeId, f: impl FnOnce(&mut ElementState)) {
    let mut state = tree.state(id);
    f(&mut state);
    tree.set_state(id, state);
}
