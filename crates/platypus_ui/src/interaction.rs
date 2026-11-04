// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Focus, keyboard and default actions on top of [`Pointer`].
//!
//! [`Interaction::handle`] takes one input, dispatches the resulting events through
//! a callback (which returns whether the script prevented the default), and then runs
//! the default actions like a browser: pressing the primary button focuses the
//! nearest focusable element (or blurs), Tab / Shift-Tab move focus (scrolling it
//! into view), the wheel and scrolling keys scroll the nearest scroll container that
//! can move, and scrollbars can be dragged.

use platypus_core::NodeId;

use crate::attrs::ClosedBy;
use crate::editor::PointerSelect;
use crate::events::{EventKind, Modifiers, Pointer, ScrollInfo, UiEvent, update_state};
use crate::tree::{ROOT, Scrollbar, UiTree};

mod dnd;
pub use dnd::DRAG_THRESHOLD;

/// Pixels per arrow-key press.
pub const SCROLL_LINE: f32 = 40.0;
/// Page Up / Page Down scroll this fraction of the visible height.
pub const SCROLL_PAGE: f32 = 0.875;

/// Where [`Input::ScrollIntoView`] puts an element in a scroll container, per axis (the
/// DOM's `ScrollLogicalPosition`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollAlign {
    /// Its start edge at the container's start.
    Start,
    /// Centred.
    Center,
    /// Its end edge at the container's end.
    End,
    /// Only as far as needed to show it (its start edge when it's bigger than the view);
    /// not at all when it's already visible.
    #[default]
    Nearest,
}

/// How a script's scroll moves (the DOM's `ScrollBehavior`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollBehavior {
    /// As the scroll container's `scroll-behavior` says.
    #[default]
    Auto,
    Instant,
    /// Eased over 250 to 500 ms ([`UiTree::smooth_scroll_to`]).
    Smooth,
}

impl ScrollBehavior {
    /// `"auto"`, `"instant"` or `"smooth"`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => Self::Auto,
            "instant" => Self::Instant,
            "smooth" => Self::Smooth,
            _ => return None,
        })
    }

    fn smooth(self, tree: &UiTree, container: NodeId) -> bool {
        match self {
            Self::Auto => tree.smooth_by_default(container),
            Self::Instant => false,
            Self::Smooth => true,
        }
    }
}

impl ScrollAlign {
    /// `"start"`, `"center"`, `"end"` or `"nearest"`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "start" => Self::Start,
            "center" => Self::Center,
            "end" => Self::End,
            "nearest" => Self::Nearest,
            _ => return None,
        })
    }

    /// How far to scroll so an item at `start` (length `len`) lands here in a view at
    /// `view_start` (length `view_len`).
    fn delta(self, start: f32, len: f32, view_start: f32, view_len: f32) -> f32 {
        let (end, view_end) = (start + len, view_start + view_len);
        match self {
            Self::Start => start - view_start,
            Self::End => end - view_end,
            Self::Center => (start + len / 2.0) - (view_start + view_len / 2.0),
            Self::Nearest if start < view_start => start - view_start,
            Self::Nearest if end > view_end => (end - view_end).min(start - view_start),
            Self::Nearest => 0.0,
        }
    }
}

/// One input for [`Interaction::handle`]. Positions and deltas are logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    PointerMove {
        x: f32,
        y: f32,
    },
    PointerLeave,
    PointerDown {
        button: u8,
        time_ms: f64,
    },
    PointerUp {
        button: u8,
    },
    Wheel {
        dx: f32,
        dy: f32,
    },
    /// `key` and `code` as in the DOM (`"a"` / `"KeyA"`); `text` is what the key types.
    KeyDown {
        key: String,
        code: String,
        repeat: bool,
        text: Option<String>,
    },
    /// IME pre-edit text for the focused field (empty ends composing); `cursor` is the
    /// IME's caret or selection as byte offsets into `text`.
    ImePreedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    /// Finished IME text to insert into the focused field.
    ImeCommit(String),
    /// Moves focus to the nearest focusable element in a direction (gamepad d-pad).
    /// Does nothing until something has focus, so the d-pad stays the game's until UI
    /// takes focus (e.g. a menu calling `ref.focus()`). The focused element first gets it as
    /// an arrow key's `keydown`; preventing that keeps focus where it is.
    Navigate(Direction),
    /// Clicks the focused element (gamepad South / A).
    Activate,
    /// Clipboard text to paste into the focused field (the answer to
    /// [`Interaction::take_clipboard`] returning [`Clipboard::Paste`]).
    Paste(String),
    KeyUp {
        key: String,
        code: String,
    },
    Modifiers(Modifiers),
    /// The window lost focus: held buttons are forgotten.
    WindowBlur,
    /// `ref.focus()` from a script.
    Focus(NodeId),
    /// `ref.blur()` from a script: blurs the element if it has focus.
    Blur(NodeId),
    /// `ref.scrollTo(x, y)` from a script (also scrolls `overflow: hidden`). A NaN
    /// coordinate keeps that axis where it is.
    ScrollTo {
        id: NodeId,
        x: f32,
        y: f32,
        behavior: ScrollBehavior,
    },
    /// `ref.scrollBy(dx, dy)` from a script.
    ScrollBy {
        id: NodeId,
        dx: f32,
        dy: f32,
        behavior: ScrollBehavior,
    },
    /// `ref.scrollIntoView(...)` from a script: where the element ends up in each of its
    /// scroll containers, vertically (`block`) and horizontally (`inline`).
    ScrollIntoView {
        id: NodeId,
        block: ScrollAlign,
        inline: ScrollAlign,
        behavior: ScrollBehavior,
    },
    /// `ref.setPointerCapture()` / `releasePointerCapture()` from a script.
    SetPointerCapture(NodeId),
    ReleasePointerCapture(NodeId),
    /// Files from the system are dragged over the window at (x, y).
    FileDragEnter {
        files: Vec<String>,
        x: f32,
        y: f32,
    },
    /// Files from the system were dropped at the pointer's position.
    FileDrop {
        files: Vec<String>,
    },
    /// The system drag left the window or was cancelled.
    FileDragCancel,
    /// `dataTransfer.setDragImage(element, x, y)` from a script during a drag: the
    /// element is drawn under the pointer, grabbed at (x, y) in it. `None`: no image.
    SetDragImage {
        id: Option<NodeId>,
        x: f32,
        y: f32,
    },
    /// The tree changed: re-hit-test the pointer and drop focus from removed or hidden
    /// elements. Call once per frame after commits.
    Refresh,
}

/// Pointer, focus and keyboard state over one [`UiTree`].
#[derive(Debug, Default)]
pub struct Interaction {
    pointer: Pointer,
    focused: Option<NodeId>,
    modifiers: Modifiers,
    /// Whether the last focus-moving input was the keyboard, which decides
    /// `:focus-visible` for focus set by scripts (the browsers' heuristic).
    keyboard: bool,
    /// A scrollbar thumb being dragged, and where on the thumb it was grabbed.
    drag: Option<(Scrollbar, f32)>,
    /// A text field being selected by dragging.
    selecting: Option<NodeId>,
    /// Static text being selected by dragging.
    selecting_text: bool,
    /// A clipboard request for the app.
    clipboard: Option<Clipboard>,
    /// Open modal dialogs, oldest first, with the element focused before each opened
    /// (focus goes back there when it closes).
    modals: Vec<(NodeId, Option<NodeId>)>,
    /// What had focus before the last change (where a modal a script focused into as it
    /// opened gives focus back).
    previous_focus: Option<NodeId>,
    /// Drag and drop: a press that may become a drag, and the drag in progress.
    dnd: dnd::Dnd,
}

/// A direction for [`Input::Navigate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// What a text field wants from the system clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clipboard {
    /// Put this text on the clipboard.
    Copy(String),
    /// Read the clipboard and send it back as [`Input::Paste`].
    Paste,
}

impl Interaction {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pointer(&self) -> &Pointer {
        &self.pointer
    }

    /// The element with keyboard focus.
    pub fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    /// The CSS cursor for the hovered element (during a drag: whether it can drop).
    pub fn cursor(&self, tree: &UiTree) -> &'static str {
        self.drag_cursor()
            .unwrap_or_else(|| self.pointer.cursor(tree))
    }

    /// The hovered element's cursor with its custom images to try first.
    pub fn cursor_with_images<'t>(
        &self,
        tree: &'t UiTree,
    ) -> (&'static str, &'t [crate::style::CursorImage]) {
        match self.drag_cursor() {
            Some(cursor) => (cursor, &[]),
            None => self.pointer.cursor_with_images(tree),
        }
    }

    /// While an editable field has focus, where the IME candidate window should go
    /// (absolute logical pixels); `None` when IME input shouldn't be allowed.
    pub fn ime_area(&self, tree: &UiTree) -> Option<crate::Rect> {
        let field = self.focused?;
        let attrs = tree.attrs(field)?;
        if attrs.disabled || attrs.read_only {
            return None;
        }
        tree.editor_ime_area(field)
    }

    /// Takes the pending clipboard request from the last key press, if any.
    pub fn take_clipboard(&mut self) -> Option<Clipboard> {
        self.clipboard.take()
    }

    /// Handles one input. `dispatch` sends an event to scripts and returns whether its
    /// default action was prevented.
    pub fn handle(
        &mut self,
        tree: &mut UiTree,
        input: Input,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        if self.dnd_input(tree, &input, dispatch) {
            return;
        }
        match input {
            Input::PointerMove { x, y } => {
                send(self.pointer.move_to(tree, x, y), dispatch);
                if let Some(field) = self.selecting {
                    tree.editor_pointer(field, x, y, PointerSelect::Drag);
                }
                if self.selecting_text {
                    tree.extend_text_selection(x, y);
                }
                if let Some((bar, grab)) = self.drag {
                    self.drag_scrollbar(tree, bar, grab, x, y, dispatch);
                }
            }
            Input::PointerLeave => send(self.pointer.leave(tree), dispatch),
            Input::PointerDown { button, time_ms } => {
                // Scrollbars take presses before the page sees them, like browsers.
                if button == 0 && self.press_scrollbar(tree, dispatch) {
                    return;
                }
                let mut pressed_nothing = button == 0;
                for event in self.pointer.down(tree, button, time_ms) {
                    let prevented = dispatch(&event);
                    if event.kind == EventKind::PointerDown && button == 0 {
                        pressed_nothing = false;
                        if !prevented {
                            let target = event
                                .path
                                .iter()
                                .copied()
                                .find(|id| focusable(tree, *id) && in_modal_scope(tree, *id));
                            self.pointer_focus(tree, target, dispatch);
                            let select = PointerSelect::Press {
                                clicks: event.detail,
                                shift: event.modifiers.shift,
                            };
                            // In a field, the press places the caret or selects;
                            // elsewhere it selects static text (or clears).
                            if let Some(field) = target.filter(|id| tree.is_editor(*id)) {
                                tree.clear_text_selection();
                                tree.editor_pointer(field, event.x, event.y, select);
                                self.selecting = Some(field);
                            } else if self.dnd_press(tree, &event) {
                                // A draggable element: moving starts a drag, not a
                                // text selection.
                                tree.clear_text_selection();
                            } else if let Some(&hit) = event.path.first() {
                                self.selecting_text =
                                    tree.select_text(hit, event.x, event.y, select);
                            }
                        }
                    }
                }
                // A press on empty space (the root) blurs, like clicking the page body.
                if pressed_nothing {
                    tree.clear_text_selection();
                    self.pointer_focus(tree, None, dispatch);
                }
            }
            Input::PointerUp { button } => {
                if button == 0 {
                    self.selecting = None;
                    self.selecting_text = false;
                    if self.drag.take().is_some() {
                        return;
                    }
                }
                for event in self.pointer.up(tree, button) {
                    let prevented = dispatch(&event);
                    if event.kind == EventKind::Click && !prevented {
                        self.backdrop_click(tree, &event, dispatch);
                    }
                }
            }
            Input::Wheel { dx, dy } => {
                // Shift turns a vertical wheel horizontal (the Windows and Linux convention).
                let (dx, dy) = if self.modifiers.shift && dx == 0.0 {
                    (dy, 0.0)
                } else {
                    (dx, dy)
                };
                for event in self.pointer.wheel(tree, dx, dy) {
                    let prevented = dispatch(&event);
                    if event.kind == EventKind::Wheel
                        && !prevented
                        && let Some(target) = event.path.first()
                    {
                        let path = tree.scroll_path(*target);
                        self.scroll_chain(tree, &path, dx, dy, dispatch);
                    }
                }
            }
            Input::KeyDown {
                key,
                code,
                repeat,
                text,
            } => {
                let prevented = self.key_event(
                    tree,
                    EventKind::KeyDown,
                    key.clone(),
                    code.clone(),
                    repeat,
                    dispatch,
                );
                if prevented {
                    return;
                }
                // The focused text field edits first.
                if let Some(field) = self.focused
                    && let Some(out) =
                        tree.editor_key(field, &key, &code, text.as_deref(), self.modifiers)
                {
                    if let Some(copy) = out.copy {
                        self.clipboard = Some(Clipboard::Copy(copy));
                    }
                    if out.paste {
                        self.clipboard = Some(Clipboard::Paste);
                    }
                    if out.changed {
                        self.input_event(tree, field, dispatch);
                    }
                    if out.handled {
                        return;
                    }
                }
                // Escape asks the top modal dialog to close.
                if key == "Escape"
                    && let Some(dialog) = tree.top_modal()
                    && closed_by(tree, dialog) != ClosedBy::None
                {
                    self.request_close(tree, dialog, dispatch);
                    return;
                }
                // Ctrl+C (Cmd+C) copies selected static text.
                let command = self.modifiers.ctrl || self.modifiers.meta;
                if command
                    && (key.eq_ignore_ascii_case("c") || code == "KeyC")
                    && let Some((_, text)) = tree.text_selection()
                {
                    self.clipboard = Some(Clipboard::Copy(text));
                    return;
                }
                if key == "Tab" {
                    self.keyboard = true;
                    let next = tab_target(tree, self.focused, self.modifiers.shift);
                    if let Some(id) = next {
                        self.set_focus(tree, next, true, dispatch);
                        self.scroll_into_view(
                            tree,
                            id,
                            ScrollAlign::Nearest,
                            ScrollAlign::Nearest,
                            ScrollBehavior::Instant,
                            dispatch,
                        );
                    }
                } else {
                    self.key_scroll(tree, &key, dispatch);
                }
            }
            Input::KeyUp { key, code } => {
                self.key_event(tree, EventKind::KeyUp, key, code, false, dispatch);
            }
            Input::Modifiers(m) => {
                self.modifiers = m;
                self.pointer.set_modifiers(m);
            }
            Input::WindowBlur => {
                self.dnd_cancel(tree, dispatch);
                self.pointer.cancel(tree);
            }
            // Handled by `dnd_input` (here only when there's nothing to do).
            Input::FileDragEnter { .. }
            | Input::FileDrop { .. }
            | Input::FileDragCancel
            | Input::SetDragImage { .. } => {}
            // Like the DOM, focusing an element that can't take focus does nothing.
            Input::Focus(id) => {
                if focusable(tree, id) {
                    let visible = self.keyboard;
                    self.set_focus(tree, Some(id), visible, dispatch);
                    self.scroll_into_view(
                        tree,
                        id,
                        ScrollAlign::Nearest,
                        ScrollAlign::Nearest,
                        ScrollBehavior::Instant,
                        dispatch,
                    );
                }
            }
            Input::ScrollTo { id, x, y, behavior } => {
                let (cx, cy) = tree.scroll_offset(id);
                let (x, y) = (
                    if x.is_nan() { cx } else { x },
                    if y.is_nan() { cy } else { y },
                );
                self.script_scroll(tree, id, x, y, behavior, dispatch);
            }
            Input::ScrollBy {
                id,
                dx,
                dy,
                behavior,
            } => {
                // From where a smooth scroll is heading, so quick steps add up.
                let (x, y) = tree.scroll_goal(id);
                self.script_scroll(tree, id, x + dx, y + dy, behavior, dispatch);
            }
            Input::ScrollIntoView {
                id,
                block,
                inline,
                behavior,
            } => self.scroll_into_view(tree, id, block, inline, behavior, dispatch),
            Input::SetPointerCapture(id) => send(self.pointer.set_capture(tree, id), dispatch),
            Input::ReleasePointerCapture(id) => {
                send(self.pointer.release_capture(tree, id), dispatch)
            }
            Input::ImePreedit { text, cursor } => {
                if let Some(field) = self.focused.filter(|_| self.ime_area(tree).is_some()) {
                    tree.editor_compose(field, &text, cursor);
                }
            }
            Input::ImeCommit(text) => {
                if let Some(field) = self.focused {
                    tree.editor_end_compose(field);
                    if tree.editor_insert(field, &text) {
                        self.input_event(tree, field, dispatch);
                    }
                }
            }
            Input::Navigate(direction) => {
                let Some(from) = self.focused else {
                    return;
                };
                // The focused element sees the d-pad as an arrow key first: one that handles
                // it (a list moving its selection) prevents the move.
                let key = match direction {
                    Direction::Up => "ArrowUp",
                    Direction::Down => "ArrowDown",
                    Direction::Left => "ArrowLeft",
                    Direction::Right => "ArrowRight",
                };
                if self.key_event(
                    tree,
                    EventKind::KeyDown,
                    key.into(),
                    key.into(),
                    false,
                    dispatch,
                ) {
                    return;
                }
                if let Some(next) = nearest_in_direction(tree, from, direction) {
                    self.keyboard = true;
                    self.set_focus(tree, Some(next), true, dispatch);
                    self.scroll_into_view(
                        tree,
                        next,
                        ScrollAlign::Nearest,
                        ScrollAlign::Nearest,
                        ScrollBehavior::Instant,
                        dispatch,
                    );
                }
            }
            Input::Activate => {
                if let Some(id) = self.focused.filter(|id| !tree.is_editor(*id)) {
                    // Like a keyboard-activated click: no pointer position, detail 0.
                    dispatch(&UiEvent::new(
                        EventKind::Click,
                        tree.event_path(id),
                        self.modifiers,
                    ));
                }
            }
            Input::Paste(text) => {
                if let Some(field) = self.focused
                    && tree.editor_insert(field, &text)
                {
                    self.input_event(tree, field, dispatch);
                }
            }
            Input::Blur(id) => {
                if self.focused == Some(id) {
                    self.set_focus(tree, None, false, dispatch);
                }
            }
            Input::Refresh => {
                // Smooth scrolls moved in `set_time`.
                for id in tree.take_smooth_scrolled() {
                    if tree.contains(id) {
                        dispatch(&UiEvent {
                            scroll: Some(ScrollInfo::of(tree, id)),
                            ..UiEvent::new(EventKind::Scroll, vec![id], self.modifiers)
                        });
                    }
                }
                send(self.pointer.refresh(tree), dispatch);
                for e in tree.take_motion_events() {
                    if !tree.contains(e.target) {
                        continue;
                    }
                    dispatch(&UiEvent {
                        value: Some(e.name),
                        delta_x: e.elapsed as f32,
                        ..UiEvent::new(e.kind, tree.event_path(e.target), self.modifiers)
                    });
                }
                self.track_modals(tree, dispatch);
                let top = tree.top_modal();
                if let Some(id) = self.focused
                    && !focusable(tree, id)
                    && Some(id) != top
                {
                    self.set_focus(tree, None, false, dispatch);
                }
            }
        }
    }

    /// Moves focus into a modal dialog that just opened (its first focusable element,
    /// else the dialog), and back to where it was when one closes.
    fn track_modals(&mut self, tree: &mut UiTree, dispatch: &mut dyn FnMut(&UiEvent) -> bool) {
        while let Some(&(dialog, before)) = self.modals.last() {
            if tree.contains(dialog) && tree.top_modal_contains(dialog) {
                break;
            }
            self.modals.pop();
            let lost = self.focused.is_none_or(|f| {
                !tree.contains(f) || !focusable(tree, f) || tree.is_inclusive_ancestor(dialog, f)
            });
            if lost {
                let back = before.filter(|b| focusable(tree, *b) && in_modal_scope(tree, *b));
                self.set_focus(tree, back, false, dispatch);
            }
        }
        if let Some(dialog) = tree.top_modal()
            && !self.modals.iter().any(|(d, _)| *d == dialog)
        {
            let inside = |id: NodeId| tree.contains(id) && tree.is_inclusive_ancestor(dialog, id);
            // A script may have focused something in the dialog as it opened (a text field
            // focusing itself): keep that, and give focus back to where it was before.
            if let Some(f) = self.focused.filter(|f| inside(*f)) {
                let before = self.previous_focus.filter(|p| !inside(*p));
                self.modals.push((dialog, before));
                let visible = self.keyboard;
                self.set_focus(tree, Some(f), visible, dispatch);
                return;
            }
            self.modals.push((dialog, self.focused));
            let first = tab_order(tree)
                .into_iter()
                .find(|id| tree.is_inclusive_ancestor(dialog, *id));
            let visible = self.keyboard;
            self.set_focus(tree, Some(first.unwrap_or(dialog)), visible, dispatch);
        }
    }

    /// A click on the top modal's `::backdrop` (outside the dialog's box) closes it
    /// when its `closedBy` is `any`.
    fn backdrop_click(
        &mut self,
        tree: &mut UiTree,
        event: &UiEvent,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        let Some(dialog) = tree.top_modal() else {
            return;
        };
        if event.path.first() != Some(&dialog) || closed_by(tree, dialog) != ClosedBy::Any {
            return;
        }
        let inside = tree
            .to_local(dialog, event.x, event.y)
            .zip(tree.rect(dialog))
            .is_some_and(|((x, y), r)| {
                x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height
            });
        if !inside {
            self.request_close(tree, dialog, dispatch);
        }
    }

    /// Sends `cancel` to a dialog and, unless that's prevented, `close`.
    fn request_close(
        &mut self,
        tree: &UiTree,
        dialog: NodeId,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        let path = tree.event_path(dialog);
        let cancel = UiEvent::new(EventKind::Cancel, path.clone(), self.modifiers);
        if !dispatch(&cancel) {
            dispatch(&UiEvent::new(EventKind::Close, path, self.modifiers));
        }
    }

    fn pointer_focus(
        &mut self,
        tree: &mut UiTree,
        target: Option<NodeId>,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        self.keyboard = false;
        self.set_focus(tree, target, false, dispatch);
    }

    /// Tells scripts that field `id`'s text changed.
    fn input_event(
        &mut self,
        tree: &UiTree,
        id: NodeId,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        dispatch(&UiEvent {
            value: tree.editor_value(id),
            ..UiEvent::new(EventKind::Input, tree.event_path(id), self.modifiers)
        });
    }

    /// Scrolls `id` to (x, y); if it moved, sends `scroll` and updates hover (content
    /// moved under the pointer). Returns whether it moved.
    fn scroll(
        &mut self,
        tree: &mut UiTree,
        id: NodeId,
        x: f32,
        y: f32,
        by_user: bool,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) -> bool {
        if !tree.scroll_to(id, x, y, by_user) {
            return false;
        }
        tree.layout_if_needed();
        dispatch(&UiEvent {
            scroll: Some(ScrollInfo::of(tree, id)),
            ..UiEvent::new(EventKind::Scroll, vec![id], self.modifiers)
        });
        send(self.pointer.refresh(tree), dispatch);
        true
    }

    /// Scrolls the first element on `path` (innermost first) that can move by
    /// (dx, dy). Returns whether anything scrolled.
    fn scroll_chain(
        &mut self,
        tree: &mut UiTree,
        path: &[NodeId],
        dx: f32,
        dy: f32,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) -> bool {
        for id in path {
            let (x, y) = tree.scroll_offset(*id);
            if self.scroll(tree, *id, x + dx, y + dy, true, dispatch) {
                return true;
            }
            // `overscroll-behavior: contain` stops the chain at this container.
            let axis = usize::from(dy != 0.0);
            if tree.contains_overscroll(*id, axis) {
                return false;
            }
        }
        false
    }

    /// Arrow keys, Page Up / Down, Space, Home and End scroll the focused element's
    /// scroll container (or the hovered one's when nothing has focus).
    fn key_scroll(
        &mut self,
        tree: &mut UiTree,
        key: &str,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        let origin = self.focused.or(self.pointer.hovered());
        let Some(origin) = origin else {
            return;
        };
        let path = tree.scroll_path(origin);
        let page = |id: &NodeId, tree: &UiTree| {
            tree.padding_box(*id)
                .map_or(0.0, |b| b.height * SCROLL_PAGE)
        };
        let shift = self.modifiers.shift;
        for id in &path {
            let (dx, dy) = match key {
                "ArrowDown" => (0.0, SCROLL_LINE),
                "ArrowUp" => (0.0, -SCROLL_LINE),
                "ArrowRight" => (SCROLL_LINE, 0.0),
                "ArrowLeft" => (-SCROLL_LINE, 0.0),
                "PageDown" => (0.0, page(id, tree)),
                "PageUp" => (0.0, -page(id, tree)),
                " " if shift => (0.0, -page(id, tree)),
                " " => (0.0, page(id, tree)),
                "Home" => (0.0, f32::NEG_INFINITY),
                "End" => (0.0, f32::INFINITY),
                _ => return,
            };
            let (x, y) = tree.scroll_offset(*id);
            if self.scroll(tree, *id, x + dx, y + dy, true, dispatch) {
                return;
            }
            if tree.contains_overscroll(*id, usize::from(dy != 0.0)) {
                return;
            }
        }
    }

    /// A script's scroll of `id` to (x, y): at once, or smoothly (`scroll` events then
    /// come as it moves, from [`Input::Refresh`]).
    fn script_scroll(
        &mut self,
        tree: &mut UiTree,
        id: NodeId,
        x: f32,
        y: f32,
        behavior: ScrollBehavior,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        if behavior.smooth(tree, id) {
            tree.smooth_scroll_to(id, x, y, false);
        } else {
            self.scroll(tree, id, x, y, false, dispatch);
        }
    }

    /// Scrolls `id`'s scroll containers (innermost first) to put it at `block` vertically
    /// and `inline` horizontally (clamped to what each can scroll), inside each one's
    /// `scroll-padding`.
    fn scroll_into_view(
        &mut self,
        tree: &mut UiTree,
        id: NodeId,
        block: ScrollAlign,
        inline: ScrollAlign,
        behavior: ScrollBehavior,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        tree.layout_if_needed();
        let ancestors: Vec<NodeId> = tree.scroll_path(id).into_iter().skip(1).collect();
        // How far smooth scrolls inside will still move the element (they start later).
        let (mut ax, mut ay) = (0.0, 0.0);
        for container in ancestors {
            if tree.scroll_axes(container, false) == [false; 2] {
                continue;
            }
            let (Some(target), Some(view)) = (tree.rect(id), tree.padding_box(container)) else {
                continue;
            };
            let [pt, pr, pb, pl] = tree.scroll_padding(container);
            let view = crate::Rect::new(
                view.x + pl,
                view.y + pt,
                (view.width - pl - pr).max(0.0),
                (view.height - pt - pb).max(0.0),
            );
            let dx = inline.delta(target.x - ax, target.width, view.x, view.width);
            let dy = block.delta(target.y - ay, target.height, view.y, view.height);
            if dx != 0.0 || dy != 0.0 {
                let (x, y) = tree.scroll_offset(container);
                if behavior.smooth(tree, container) {
                    if let Some((tx, ty)) = tree.smooth_scroll_to(container, x + dx, y + dy, false)
                    {
                        ax += tx - x;
                        ay += ty - y;
                    }
                } else {
                    self.scroll(tree, container, x + dx, y + dy, false, dispatch);
                }
            }
        }
    }

    /// A primary press on a scrollbar: grabs the thumb, or pages toward the press on
    /// the track. Returns whether the press hit a scrollbar.
    fn press_scrollbar(
        &mut self,
        tree: &mut UiTree,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) -> bool {
        let Some((x, y)) = self.pointer.position() else {
            return false;
        };
        let path = tree
            .hit_test(x, y)
            .map(|t| tree.scroll_path(t))
            .unwrap_or_default();
        let Some(bar) = tree.scrollbar_at(&path, x, y) else {
            return false;
        };
        let (along, thumb_start, thumb_len) = if bar.vertical {
            (y, bar.thumb.y, bar.thumb.height)
        } else {
            (x, bar.thumb.x, bar.thumb.width)
        };
        if along >= thumb_start && along < thumb_start + thumb_len {
            self.drag = Some((bar, along - thumb_start));
        } else {
            let view = tree.padding_box(bar.node).unwrap_or_default();
            let page = if bar.vertical {
                view.height
            } else {
                view.width
            } * SCROLL_PAGE;
            let step = if along < thumb_start { -page } else { page };
            let (sx, sy) = tree.scroll_offset(bar.node);
            let (nx, ny) = if bar.vertical {
                (sx, sy + step)
            } else {
                (sx + step, sy)
            };
            self.scroll(tree, bar.node, nx, ny, true, dispatch);
        }
        true
    }

    /// Moves a dragged thumb so the grabbed point follows the pointer.
    fn drag_scrollbar(
        &mut self,
        tree: &mut UiTree,
        bar: Scrollbar,
        grab: f32,
        x: f32,
        y: f32,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        let (along, track_start, track_len, thumb_len) = if bar.vertical {
            (y, bar.track.y, bar.track.height, bar.thumb.height)
        } else {
            (x, bar.track.x, bar.track.width, bar.thumb.width)
        };
        let travel = track_len - thumb_len;
        if travel <= 0.0 {
            return;
        }
        let (max_x, max_y) = tree.scroll_max(bar.node);
        let max = if bar.vertical { max_y } else { max_x };
        let offset = (along - grab - track_start) / travel * max;
        let (sx, sy) = tree.scroll_offset(bar.node);
        let (nx, ny) = if bar.vertical {
            (sx, offset)
        } else {
            (offset, sy)
        };
        self.scroll(tree, bar.node, nx, ny, true, dispatch);
    }

    /// Sends a key event to the focused element. Returns whether it was prevented.
    fn key_event(
        &mut self,
        tree: &UiTree,
        kind: EventKind,
        key: String,
        code: String,
        repeat: bool,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) -> bool {
        let Some(focused) = self.focused else {
            return false;
        };
        let event = UiEvent {
            key,
            code,
            repeat,
            selection: tree.editor_selection_range(focused).unwrap_or_default(),
            ..UiEvent::new(kind, tree.event_path(focused), self.modifiers)
        };
        dispatch(&event)
    }

    /// Moves focus to `target` (or nowhere), updating `:focus` / `:focus-visible` and
    /// sending `focusout` then `focusin`.
    fn set_focus(
        &mut self,
        tree: &mut UiTree,
        target: Option<NodeId>,
        visible: bool,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        let old = self.focused;
        if let Some(field) = old.filter(|id| Some(*id) != target) {
            // Leaving a field mid-composition drops the IME's pre-edit text.
            tree.editor_end_compose(field);
        }
        if old == target {
            // Re-focusing by keyboard still shows the ring.
            if let Some(id) = target {
                update_state(tree, id, |s| s.focus_visible = visible);
            }
            return;
        }
        self.focused = target;
        self.previous_focus = old;
        if let Some(id) = old.filter(|id| tree.contains(*id)) {
            update_state(tree, id, |s| {
                s.focus = false;
                s.focus_visible = false;
            });
            dispatch(&UiEvent {
                related: target,
                ..UiEvent::new(EventKind::FocusOut, tree.event_path(id), self.modifiers)
            });
        }
        if let Some(id) = target {
            update_state(tree, id, |s| {
                s.focus = true;
                s.focus_visible = visible;
            });
            dispatch(&UiEvent {
                related: old,
                ..UiEvent::new(EventKind::FocusIn, tree.event_path(id), self.modifiers)
            });
        }
    }
}

fn send(events: Vec<UiEvent>, dispatch: &mut dyn FnMut(&UiEvent) -> bool) {
    for event in &events {
        dispatch(event);
    }
}

/// A dialog's `closedBy`, defaulting to `closerequest` (Escape only) like HTML's modals.
fn closed_by(tree: &UiTree, dialog: NodeId) -> ClosedBy {
    tree.attrs(dialog)
        .and_then(|a| a.closed_by)
        .unwrap_or(ClosedBy::CloseRequest)
}

/// Whether `id` can be reached while a modal dialog is open: everything outside the
/// top modal is inert.
fn in_modal_scope(tree: &UiTree, id: NodeId) -> bool {
    tree.top_modal()
        .is_none_or(|dialog| tree.is_inclusive_ancestor(dialog, id))
}

/// `id`'s tab index: its `tabIndex`, or 0 for text fields, which are focusable by
/// default like in HTML.
fn tab_index(tree: &UiTree, id: NodeId) -> Option<i32> {
    let attrs = tree.attrs(id)?;
    attrs.tab_index.or(tree.is_editor(id).then_some(0))
}

/// Whether `id` can take focus: it has a tab index, isn't disabled and is displayed.
pub fn focusable(tree: &UiTree, id: NodeId) -> bool {
    tab_index(tree, id).is_some()
        && tree.attrs(id).is_some_and(|a| !a.disabled)
        && tree.is_displayed(id)
        && tree.computed_text(id).visible
}

/// The Tab order: positive `tabIndex` values first (ascending, ties in tree order),
/// then `tabIndex` 0 in tree order. Negative values are skipped.
pub fn tab_order(tree: &UiTree) -> Vec<NodeId> {
    let mut order = Vec::new();
    let mut stack = vec![ROOT];
    while let Some(id) = stack.pop() {
        if id != ROOT {
            if !tree.is_displayed(id) {
                continue;
            }
            if let Some(attrs) = tree.attrs(id)
                && let Some(index) = tab_index(tree, id)
                && index >= 0
                && !attrs.disabled
            {
                order.push((index, id));
            }
        }
        stack.extend(
            tree.children(id)
                .iter()
                .rev()
                .filter(|c| !tree.is_generated(**c)),
        );
    }
    // Stable, so equal indexes keep tree order; 0 sorts after every positive index.
    order.sort_by_key(|(index, _)| if *index == 0 { i32::MAX } else { *index });
    order
        .into_iter()
        .map(|(_, id)| id)
        .filter(|id| in_modal_scope(tree, *id))
        .collect()
}

/// The focusable element (in the Tab order) nearest to `from` in `direction`: its
/// centre must lie that way, and distance across the direction counts double, so
/// elements in line win over closer ones off to the side.
pub fn nearest_in_direction(tree: &UiTree, from: NodeId, direction: Direction) -> Option<NodeId> {
    let center = |id: NodeId| {
        tree.rect(id)
            .map(|r| (r.x + r.width / 2.0, r.y + r.height / 2.0))
    };
    let (fx, fy) = center(from)?;
    tab_order(tree)
        .into_iter()
        .filter(|id| *id != from)
        .filter_map(|id| {
            let (cx, cy) = center(id)?;
            let (dx, dy) = (cx - fx, cy - fy);
            let (along, across) = match direction {
                Direction::Right => (dx, dy),
                Direction::Left => (-dx, dy),
                Direction::Down => (dy, dx),
                Direction::Up => (-dy, dx),
            };
            (along > 0.5).then_some((along + 2.0 * across.abs(), id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// The element Tab (or Shift-Tab with `backwards`) moves to from `from`, wrapping
/// around. From nothing (or an element outside the order) it starts at an end.
fn tab_target(tree: &UiTree, from: Option<NodeId>, backwards: bool) -> Option<NodeId> {
    let order = tab_order(tree);
    let last = order.len().checked_sub(1)?;
    let at = from.and_then(|f| order.iter().position(|id| *id == f));
    let next = match (at, backwards) {
        (Some(i), false) => (i + 1) % order.len(),
        (Some(i), true) => i.checked_sub(1).unwrap_or(last),
        (None, false) => 0,
        (None, true) => last,
    };
    Some(order[next])
}
