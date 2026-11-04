// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Text fields: `<input>` (one line) and `<textarea>` (wrapping lines), each backed by
//! a Parley `PlainEditor` that owns the text, the selection and IME composition
//! (DESIGN D7). The tree feeds it styles and sizes; [`crate::Interaction`] feeds it
//! keys and pointer presses; scripts see `input` events and control it through the
//! `value` attribute.

use parley::{AlignmentOptions, Layout, PlainEditor, StyleProperty, StyleSet};

use crate::Rgba;
use crate::draw::Rect;
use crate::events::Modifiers;
use crate::text::{self, Brush, TextEngine};
use crate::text_style::{ComputedText, Direction, TextAlign};
use crate::tree::PseudoStyle;

/// Default width of an `<input>` without one, about 20 characters (like browsers).
pub const INPUT_WIDTH: f32 = 160.0;
/// Default width of a `<textarea>` without one.
pub const TEXTAREA_WIDTH: f32 = 240.0;
/// Width of one character for `cols`, so that 20 characters make an [`INPUT_WIDTH`].
pub const CHAR_WIDTH: f32 = 8.0;
/// Visible rows of a `<textarea>` without `rows`.
pub const TEXTAREA_ROWS: u32 = 2;
/// The caret blinks on and off this often (ms); typing restarts it.
pub const CARET_BLINK_MS: f64 = 530.0;
/// Caret width in logical pixels.
pub const CARET_WIDTH: f32 = 1.5;
/// What a password field shows for each character.
const MASK: char = '\u{2022}';
/// Selected text background, when the field has focus.
pub const SELECTION: Rgba = Rgba {
    r: 0.2,
    g: 0.53,
    b: 1.0,
    a: 0.4,
};

/// Element types that are text fields.
pub fn is_editor_kind(kind: &str) -> bool {
    matches!(kind, "input" | "textarea")
}

/// What a key press did to a field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyOutcome {
    /// The field used the key (so it shouldn't also move focus or scroll).
    pub handled: bool,
    /// The text changed (send an `input` event).
    pub changed: bool,
    /// Text to put on the clipboard (Ctrl+C, or Ctrl+X, which also deleted it).
    pub copy: Option<String>,
    /// Ctrl+V: the app should paste the clipboard's text.
    pub paste: bool,
}

/// How a pointer press selects: by caret, word or line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerSelect {
    /// A press: `clicks` 1 places the caret (extends with Shift), 2 selects a word,
    /// 3 or more a line.
    Press { clicks: u32, shift: bool },
    /// Dragging after a press.
    Drag,
}

pub(crate) struct Editor {
    pub(crate) editor: PlainEditor<Brush>,
    pub(crate) multiline: bool,
    /// Visible rows of a textarea (`rows`).
    pub(crate) rows: u32,
    /// Width of a textarea in characters (`cols`).
    pub(crate) cols: Option<u32>,
    /// The style the editor was last given, to restyle only on change.
    style: Option<ComputedText>,
    /// The placeholder text and its laid-out form (dimmed text colour).
    placeholder: Option<(String, Option<Layout<Brush>>)>,
    /// What `::placeholder` rules set.
    placeholder_style: Option<PseudoStyle>,
    /// How far the text is scrolled inside the content box (keeps the caret in view).
    pub(crate) scroll: (f32, f32),
    /// Content box size from the last layout.
    pub(crate) size: (f32, f32),
    /// When the caret last moved or the text changed, to restart the blink.
    activity_ms: f64,
    /// A script set the text: move the caret to the end at the next layout, like browsers.
    caret_to_end: bool,
    /// `type="password"`: the Parley editor holds one [`MASK`] per character and the real
    /// text is kept in `secret`.
    password: bool,
    secret: String,
    /// What the `type` attribute makes of a single-line field besides `password`.
    kind: InputKind,
    /// `min`, `max` and `step` of a `number` field.
    pub(crate) min: Option<f64>,
    pub(crate) max: Option<f64>,
    pub(crate) step: Option<f64>,
    /// `maxLength`, in characters.
    pub(crate) max_length: Option<usize>,
}

/// The `type`s of an `<input>` that change how it takes text; the rest are plain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InputKind {
    #[default]
    Text,
    /// Digits, signs, `.` and `e`; ArrowUp/ArrowDown step the value.
    Number,
    /// Escape clears it.
    Search,
}

impl Editor {
    pub(crate) fn new(multiline: bool) -> Self {
        Self {
            editor: PlainEditor::new(16.0),
            multiline,
            rows: TEXTAREA_ROWS,
            cols: None,
            style: None,
            placeholder: None,
            placeholder_style: None,
            scroll: (0.0, 0.0),
            size: (0.0, 0.0),
            activity_ms: 0.0,
            caret_to_end: false,
            password: false,
            secret: String::new(),
            kind: InputKind::Text,
            min: None,
            max: None,
            step: None,
            max_length: None,
        }
    }

    /// Applies an `<input>`'s `type` (`None` for a textarea or no type).
    pub(crate) fn set_type(&mut self, input_type: Option<&str>) {
        self.set_password(input_type == Some("password"));
        self.kind = match input_type {
            Some("number") => InputKind::Number,
            Some("search") => InputKind::Search,
            _ => InputKind::Text,
        };
    }

    /// The text a field takes from `s` typed or pasted at the selection: a `number` field
    /// keeps only the characters of a number, and `maxLength` cuts what doesn't fit.
    fn accept(&self, s: &str) -> String {
        let mut out: String = if self.kind == InputKind::Number {
            s.chars()
                .filter(|c| c.is_ascii_digit() || "+-.eE".contains(*c))
                .collect()
        } else {
            s.to_owned()
        };
        if let Some(max) = self.max_length {
            let (from, to) = self.selection_chars();
            let len = if self.password {
                self.secret.chars().count()
            } else {
                self.editor.raw_text().chars().count()
            };
            let room = max.saturating_sub(len - (to - from));
            out = out.chars().take(room).collect();
        }
        out
    }

    /// A `number` field's value one step up or down, kept on the step grid that starts at
    /// `min` (or 0) and within `min`..`max`. An empty or unreadable field counts as 0.
    fn stepped(&self, up: bool) -> String {
        let step = self.step.filter(|s| *s > 0.0).unwrap_or(1.0);
        let base = self.min.unwrap_or(0.0);
        let now = self.text().trim().parse::<f64>().unwrap_or(0.0);
        let cells = (now - base) / step;
        let cells = if up {
            (cells + 1e-9).floor() + 1.0
        } else {
            (cells - 1e-9).ceil() - 1.0
        };
        let mut next = base + cells * step;
        if let Some(max) = self.max {
            next = next.min(max);
        }
        if let Some(min) = self.min {
            next = next.max(min);
        }
        // Whole numbers without a point, and no float noise (0.1 + 0.2).
        let next = (next * 1e12).round() / 1e12 + 0.0;
        let shown = format!("{next:.12}");
        shown.trim_end_matches('0').trim_end_matches('.').to_owned()
    }

    /// Switches `type="password"` on or off, re-showing the text masked or plain.
    pub(crate) fn set_password(&mut self, on: bool) {
        if self.password == on || self.multiline {
            return;
        }
        if on {
            self.secret = self.editor.raw_text().to_owned();
            self.password = true;
            self.show_masked();
        } else {
            let plain = std::mem::take(&mut self.secret);
            self.password = false;
            self.editor.set_text(&plain);
        }
        self.caret_to_end = true;
    }

    /// Puts one mask per character of `secret` into the editor, keeping the selection's
    /// offsets (clamped).
    fn show_masked(&mut self) {
        let masked: String = self.secret.chars().map(|_| MASK).collect();
        self.editor.set_text(&masked);
    }

    /// After an edit of a password field (the editor now holds the typed characters among
    /// the masks): updates `secret` and masks the editor again. `old` is the selection's
    /// character range before the edit, `n` the length of the secret before it.
    fn absorb_edit(&mut self, text: &mut TextEngine, old: (usize, usize), n: usize) {
        let shown = self.editor.raw_text();
        let new_len = shown.chars().count();
        let focus = self.editor.raw_selection().focus().index();
        let caret = shown[..focus.min(shown.len())].chars().count();
        let (start, _) = old;
        let secret: Vec<char> = self.secret.chars().collect();
        let (from, typed, removed) = if caret >= start {
            // Typed or pasted text sits between `start` and the caret; what the edit took
            // out (a selection, or a character for Delete) starts at `start` too.
            let typed: String = shown.chars().skip(start).take(caret - start).collect();
            (start, typed, (n + (caret - start)).saturating_sub(new_len))
        } else {
            // Backspace: the caret went back over what was removed.
            (caret, String::new(), n.saturating_sub(new_len))
        };
        let mut next: String = secret.iter().take(from).collect();
        next.push_str(&typed);
        next.extend(secret.iter().skip((from + removed).min(n)));
        self.secret = next;
        self.show_masked();
        let (font_cx, layout_cx) = text.contexts();
        let at = (from + typed.chars().count()) * MASK.len_utf8();
        self.editor.driver(font_cx, layout_cx).move_to_byte(at);
    }

    /// The selection as character offsets (masks are one character each).
    fn selection_chars(&self) -> (usize, usize) {
        let r = self.editor.raw_selection().text_range();
        let count = |b: usize| {
            self.editor.raw_text()[..b.min(self.editor.raw_text().len())]
                .chars()
                .count()
        };
        (count(r.start), count(r.end))
    }

    /// Content size when the element's size is auto: a default width, and one line
    /// (or `rows` lines) of height.
    pub(crate) fn intrinsic_size(&self, known_width: Option<f32>) -> (f32, f32) {
        let (width, lines) = if self.multiline {
            (
                self.cols.map_or(TEXTAREA_WIDTH, |c| c as f32 * CHAR_WIDTH),
                self.rows,
            )
        } else {
            (INPUT_WIDTH, 1)
        };
        (
            known_width.unwrap_or(width),
            (self.line_height() * lines as f32).ceil(),
        )
    }

    /// The field's value (the real text of a password field, not its masks).
    pub(crate) fn text(&self) -> String {
        if self.password {
            return self.secret.clone();
        }
        self.editor.text().to_string()
    }

    /// Replaces the text (from the `value` attribute) if it differs, keeping the
    /// selection where it can.
    pub(crate) fn set_value(&mut self, value: &str) -> bool {
        if self.editor.is_composing() {
            return false;
        }
        if self.password {
            if self.secret == value {
                return false;
            }
            self.secret = value.to_owned();
            self.show_masked();
        } else {
            if self.editor.raw_text() == value {
                return false;
            }
            self.editor.set_text(value);
        }
        self.caret_to_end = true;
        true
    }

    pub(crate) fn set_placeholder(&mut self, placeholder: Option<&str>) {
        let same = self.placeholder.as_ref().map(|(p, _)| p.as_str()) == placeholder;
        if !same {
            self.placeholder = placeholder.map(|p| (p.to_owned(), None));
        }
    }

    /// Applies `::placeholder` styles.
    pub(crate) fn set_placeholder_style(&mut self, style: Option<PseudoStyle>) {
        if self.placeholder_style != style {
            self.placeholder_style = style;
            if let Some((_, layout)) = &mut self.placeholder {
                *layout = None;
            }
        }
    }

    /// Applies the computed text style; returns whether it changed.
    pub(crate) fn set_style(&mut self, style: &ComputedText) -> bool {
        if self.style.as_ref() == Some(style) {
            return false;
        }
        let set = self.editor.edit_styles();
        *set = style_set(style);
        self.editor
            .set_base_direction(text::base_direction(style.direction));
        self.editor.set_alignment(text::alignment(style.text_align));
        if let Some((_, layout)) = &mut self.placeholder {
            *layout = None;
        }
        self.style = Some(style.clone());
        true
    }

    /// Lays out the text for a content box of `width` × `height` and keeps the caret
    /// in view.
    pub(crate) fn refresh(&mut self, text: &mut TextEngine, width: f32, height: f32) {
        if self.size != (width, height)
            && let Some((_, layout)) = &mut self.placeholder
        {
            // Wrapped and aligned for the old size.
            *layout = None;
        }
        self.size = (width, height);
        self.editor
            .set_width(self.multiline.then_some(width.max(1.0)));
        let (font_cx, layout_cx) = text.contexts();
        if std::mem::take(&mut self.caret_to_end) {
            self.editor.driver(font_cx, layout_cx).move_to_text_end();
        }
        self.editor.refresh_layout(font_cx, layout_cx);
        if let (Some((placeholder, layout @ None)), Some(style)) =
            (&mut self.placeholder, &self.style)
        {
            // Browsers dim the placeholder; `::placeholder` can restyle it.
            let mut dim = style.clone();
            dim.color = dim.color.with_alpha_scaled(0.5);
            if let Some(p) = &self.placeholder_style {
                dim = dim.child(&p.text);
                if let Some(opacity) = p.opacity {
                    dim.color = dim.color.with_alpha_scaled(opacity.clamp(0.0, 1.0));
                }
            }
            let mut built = text.build(&dim, &[(placeholder.clone(), dim.clone())]);
            built.break_all_lines(self.multiline.then_some(width.max(1.0)));
            built.align(
                text::alignment(style.text_align),
                AlignmentOptions::default(),
            );
            *layout = Some(built);
        }
        self.keep_caret_visible();
    }

    pub(crate) fn layout(&self) -> Option<&Layout<Brush>> {
        self.editor.try_layout()
    }

    /// How far the placeholder is shifted right of the text's origin (which already
    /// includes the scroll): an input lays out unbounded, so it is aligned here.
    pub(crate) fn placeholder_x(&self) -> f32 {
        match self.placeholder_layout() {
            Some(l) if !self.multiline => {
                (self.size.0 - l.full_width()).max(0.0) * self.align_factor() + self.scroll.0
            }
            _ => 0.0,
        }
    }

    /// Where a line narrower than an input goes: 0 at the left, 1 at the right.
    fn align_factor(&self) -> f32 {
        let Some(style) = &self.style else {
            return 0.0;
        };
        let rtl = style.direction == Direction::Rtl;
        match (style.text_align, rtl) {
            (TextAlign::Left, _) | (TextAlign::Start | TextAlign::Justify, false) => 0.0,
            (TextAlign::End, true) => 0.0,
            (TextAlign::Center, _) => 0.5,
            _ => 1.0,
        }
    }

    /// The placeholder's layout, shown while the field is empty.
    pub(crate) fn placeholder_layout(&self) -> Option<&Layout<Brush>> {
        if !self.editor.raw_text().is_empty() {
            return None;
        }
        self.placeholder.as_ref().and_then(|(_, l)| l.as_ref())
    }

    /// The height of one line: the layout's, or an estimate before there is any text.
    pub(crate) fn line_height(&self) -> f32 {
        self.layout()
            .and_then(|l| l.lines().next().map(|line| line.metrics().line_height))
            .filter(|h| *h > 0.0)
            .unwrap_or_else(|| {
                self.style.as_ref().map_or(16.0 * 1.25, |s| {
                    s.line_height_px().unwrap_or(s.font_size * 1.25)
                })
            })
    }

    /// Scrolls so the caret stays inside the content box.
    fn keep_caret_visible(&mut self) {
        let Some(layout) = self.layout() else {
            return;
        };
        let content = (layout.full_width(), layout.height());
        let Some(caret) = self.editor.cursor_geometry(CARET_WIDTH) else {
            return;
        };
        let (w, h) = self.size;
        let fit = |scroll: f32, start: f32, end: f32, view: f32, content: f32| {
            let mut s = scroll;
            if end - s > view {
                s = end - view;
            }
            if start - s < 0.0 {
                s = start;
            }
            s.clamp(0.0, (content + CARET_WIDTH - view).max(0.0))
        };
        if self.multiline {
            self.scroll.1 = fit(
                self.scroll.1,
                caret.y0 as f32,
                caret.y1 as f32,
                h,
                content.1,
            );
            self.scroll.0 = 0.0;
        } else {
            self.scroll.0 = fit(
                self.scroll.0,
                caret.x0 as f32,
                caret.x1 as f32,
                w,
                content.0,
            );
            // Text that fits is aligned by scrolling it back (negative scroll), so
            // drawing, hit testing and the IME area all follow.
            let free = w - content.0 - CARET_WIDTH;
            if free > 0.0 {
                self.scroll.0 = -free * self.align_factor();
            }
            self.scroll.1 = 0.0;
        }
    }

    pub(crate) fn touch(&mut self, now_ms: f64) {
        self.activity_ms = now_ms;
    }

    /// Whether the caret is in the "on" half of its blink.
    pub(crate) fn caret_on(&self, now_ms: f64) -> bool {
        (((now_ms - self.activity_ms).max(0.0) / CARET_BLINK_MS) as u64).is_multiple_of(2)
    }

    /// Selection rectangles in content-box coordinates (before scrolling).
    pub(crate) fn selection_rects(&self) -> Vec<Rect> {
        self.editor
            .selection_geometry()
            .into_iter()
            .map(|(b, _)| bbox(b))
            .collect()
    }

    pub(crate) fn caret_rect(&self) -> Option<Rect> {
        self.editor.cursor_geometry(CARET_WIDTH).map(bbox)
    }

    /// The text area the IME candidate window should avoid, in content-box coordinates.
    pub(crate) fn ime_area(&self) -> Rect {
        bbox(self.editor.ime_cursor_area())
    }

    /// Runs a key's editing action (`key` and `code` as in the DOM). `read_only` fields
    /// still move and select.
    pub(crate) fn key(
        &mut self,
        text: &mut TextEngine,
        key: &str,
        code: &str,
        typed: Option<&str>,
        m: Modifiers,
        read_only: bool,
    ) -> KeyOutcome {
        // Fields with keys of their own: a number steps, a search clears.
        let own = match (self.kind, key) {
            (InputKind::Number, "ArrowUp" | "ArrowDown") => Some(self.stepped(key == "ArrowUp")),
            (InputKind::Search, "Escape") if !self.editor.raw_text().is_empty() => {
                Some(String::new())
            }
            _ => None,
        };
        if let Some(value) = own.filter(|_| !read_only) {
            let changed = self.set_value(&value);
            return KeyOutcome {
                handled: true,
                changed,
                ..KeyOutcome::default()
            };
        }
        let typed = typed.map(|t| self.accept(t));
        let typed = typed.as_deref();
        let (font_cx, layout_cx) = text.contexts();
        let before = self.editor.generation();
        let pw = self.password;
        let old_text = if pw {
            self.secret.clone()
        } else {
            self.editor.raw_text().to_owned()
        };
        let (old_sel, old_len) = (self.selection_chars(), self.secret.chars().count());
        let mut mutated = false;
        let mut d = self.editor.driver(font_cx, layout_cx);
        // Ctrl on Windows and Linux, Command on macOS.
        let command = m.ctrl || m.meta;
        // AltGr arrives as Ctrl+Alt and types, so it isn't a shortcut.
        let key = if command && !(m.ctrl && m.alt) {
            shortcut_key(key, code)
        } else {
            key
        };
        let mut out = KeyOutcome {
            handled: true,
            ..KeyOutcome::default()
        };
        let edit = !read_only;
        match key {
            // A password has no words: Ctrl moves and deletes to the ends.
            "ArrowLeft" | "ArrowRight" if pw && command => match (key == "ArrowLeft", m.shift) {
                (true, true) => d.select_to_text_start(),
                (true, false) => d.move_to_text_start(),
                (false, true) => d.select_to_text_end(),
                (false, false) => d.move_to_text_end(),
            },
            "Backspace" | "Delete" if pw && command && edit => {
                if key == "Backspace" {
                    d.select_to_text_start();
                } else {
                    d.select_to_text_end();
                }
                d.delete_selection();
                mutated = true;
            }
            "ArrowLeft" => match (m.shift, command) {
                (true, true) => d.select_word_left(),
                (true, false) => d.select_left(),
                (false, true) => d.move_word_left(),
                (false, false) => d.move_left(),
            },
            "ArrowRight" => match (m.shift, command) {
                (true, true) => d.select_word_right(),
                (true, false) => d.select_right(),
                (false, true) => d.move_word_right(),
                (false, false) => d.move_right(),
            },
            "ArrowUp" if self.multiline => {
                if m.shift {
                    d.select_up()
                } else {
                    d.move_up()
                }
            }
            "ArrowDown" if self.multiline => {
                if m.shift {
                    d.select_down()
                } else {
                    d.move_down()
                }
            }
            // One line: up and down go to the start and end.
            "ArrowUp" | "Home" if command || !self.multiline => {
                if m.shift {
                    d.select_to_text_start()
                } else {
                    d.move_to_text_start()
                }
            }
            "ArrowDown" | "End" if command || !self.multiline => {
                if m.shift {
                    d.select_to_text_end()
                } else {
                    d.move_to_text_end()
                }
            }
            "Home" => {
                if m.shift {
                    d.select_to_line_start()
                } else {
                    d.move_to_line_start()
                }
            }
            "End" => {
                if m.shift {
                    d.select_to_line_end()
                } else {
                    d.move_to_line_end()
                }
            }
            "Backspace" if edit => {
                mutated = true;
                if command {
                    d.backdelete_word()
                } else {
                    d.backdelete()
                }
            }
            "Delete" if edit => {
                mutated = true;
                if command { d.delete_word() } else { d.delete() }
            }
            "Enter" if edit && self.multiline => d.insert_or_replace_selection("\n"),
            "a" | "A" if command => d.select_all(),
            // A password gives nothing to the clipboard, and cutting it does nothing.
            "c" | "C" if command => {
                out.copy = d.editor.selected_text().filter(|_| !pw).map(str::to_owned)
            }
            "x" | "X" if command => {
                out.copy = d.editor.selected_text().filter(|_| !pw).map(str::to_owned);
                if edit && out.copy.is_some() {
                    d.delete_selection();
                }
            }
            "v" | "V" if command && edit => out.paste = true,
            _ => {
                // Typed characters (AltGr arrives as Ctrl+Alt and still types).
                let typing = !command || (m.ctrl && m.alt);
                match typed.filter(|t| typing && !t.is_empty()) {
                    Some(t) if edit => {
                        let t = if self.multiline {
                            t.to_owned()
                        } else {
                            t.replace(['\n', '\r'], "")
                        };
                        d.insert_or_replace_selection(&t);
                        mutated = true;
                    }
                    Some(_) => {}
                    None => out.handled = false,
                }
            }
        }
        if pw && mutated {
            self.absorb_edit(text, old_sel, old_len);
        }
        out.changed = if pw {
            self.secret != old_text
        } else {
            self.editor.raw_text() != old_text
        };
        if out.handled || self.editor.generation() != before {
            self.keep_caret_visible();
        }
        out
    }

    /// Inserts text at the caret, replacing the selection (paste, IME commit).
    /// Single-line fields join lines with spaces and drop a trailing line break (copied
    /// lines often end in one). Returns whether the text changed.
    pub(crate) fn insert(&mut self, text: &mut TextEngine, s: &str) -> bool {
        let s = if self.multiline {
            s.to_owned()
        } else {
            s.trim_end_matches(['\n', '\r'])
                .replace("\r\n", " ")
                .replace(['\n', '\r'], " ")
        };
        let s = self.accept(&s);
        let old = if self.password {
            self.secret.clone()
        } else {
            self.editor.raw_text().to_owned()
        };
        let (old_sel, old_len) = (self.selection_chars(), self.secret.chars().count());
        let (font_cx, layout_cx) = text.contexts();
        self.editor
            .driver(font_cx, layout_cx)
            .insert_or_replace_selection(&s);
        if self.password {
            self.absorb_edit(text, old_sel, old_len);
        }
        self.keep_caret_visible();
        if self.password {
            self.secret != old
        } else {
            self.editor.raw_text() != old
        }
    }

    /// Places the caret or selects at (x, y) in content-box coordinates.
    pub(crate) fn pointer(&mut self, text: &mut TextEngine, x: f32, y: f32, select: PointerSelect) {
        let (x, y) = (x + self.scroll.0, y + self.scroll.1);
        let (font_cx, layout_cx) = text.contexts();
        let mut d = self.editor.driver(font_cx, layout_cx);
        match select {
            PointerSelect::Press {
                clicks: 1,
                shift: true,
            } => d.shift_click_extension(x, y),
            PointerSelect::Press { clicks: 0 | 1, .. } => d.move_to_point(x, y),
            // The whole password is one word.
            PointerSelect::Press { clicks: 2, .. } if self.password => d.select_all(),
            PointerSelect::Press { clicks: 2, .. } => d.select_word_at_point(x, y),
            PointerSelect::Press { .. } => {
                if self.multiline {
                    d.select_hard_line_at_point(x, y)
                } else {
                    d.select_all()
                }
            }
            PointerSelect::Drag => d.extend_selection_to_point(x, y),
        }
        self.keep_caret_visible();
    }

    /// The selection as character offsets into the text, `(start, end)` (equal for a bare caret).
    pub(crate) fn selection_range(&self) -> (usize, usize) {
        let s = self.editor.raw_selection();
        let (a, b) = (s.anchor().index(), s.focus().index());
        let text = self.editor.raw_text();
        let chars = |byte: usize| text.get(..byte).map_or(byte, |t| t.chars().count());
        (chars(a.min(b)), chars(a.max(b)))
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        if self.password {
            return None;
        }
        self.editor.selected_text().map(str::to_owned)
    }

    /// IME pre-edit: shows `preedit` at the caret (an empty string ends composing).
    /// `cursor` is the IME's caret/selection as byte offsets into `preedit`.
    pub(crate) fn compose(
        &mut self,
        text: &mut TextEngine,
        preedit: &str,
        cursor: Option<(usize, usize)>,
    ) {
        let (font_cx, layout_cx) = text.contexts();
        let mut d = self.editor.driver(font_cx, layout_cx);
        if preedit.is_empty() {
            d.clear_compose();
        } else {
            d.set_compose(preedit, cursor);
        }
        self.keep_caret_visible();
    }

    /// Ends IME composition, dropping any pre-edit text (the commit arrives separately).
    pub(crate) fn end_compose(&mut self, text: &mut TextEngine) {
        if self.editor.is_composing() {
            let (font_cx, layout_cx) = text.contexts();
            self.editor.driver(font_cx, layout_cx).clear_compose();
        }
    }
}

/// The key a Ctrl/Cmd shortcut matches on: `key` when it's a Latin letter, else the
/// letter of the physical `code`, so Ctrl+A still selects all on a Russian layout
/// (`key` "ф", `code` "KeyA").
fn shortcut_key<'a>(key: &'a str, code: &'a str) -> &'a str {
    if key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() {
        return key;
    }
    match code.strip_prefix("Key") {
        Some(letter) if letter.len() == 1 => letter,
        _ => key,
    }
}

fn bbox(b: parley::BoundingBox) -> Rect {
    Rect::new(
        b.x0 as f32,
        b.y0 as f32,
        (b.x1 - b.x0) as f32,
        (b.y1 - b.y0) as f32,
    )
}

/// The editor's single style, from the element's computed text style.
fn style_set(s: &ComputedText) -> StyleSet<Brush> {
    let base = crate::text::to_parley(s);
    let mut set = StyleSet::new(s.font_size);
    set.insert(StyleProperty::FontFamily(base.font_family));
    set.insert(StyleProperty::FontStyle(base.font_style));
    set.insert(StyleProperty::FontWeight(base.font_weight));
    set.insert(StyleProperty::Brush(crate::text::Brush::of(s)));
    set.insert(StyleProperty::LineHeight(base.line_height));
    set.insert(StyleProperty::LetterSpacing(s.letter_spacing));
    set.insert(StyleProperty::WordSpacing(s.word_spacing));
    set
}
