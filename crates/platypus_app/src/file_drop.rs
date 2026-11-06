// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Files dragged onto the window from the system. winit reports one event per file
//! (`HoveredFile`, `DroppedFile`, `HoveredFileCancelled`) and no positions while the
//! system drags, so the files are gathered here and handed to the screen's UI once a
//! frame as drag inputs, at the cursor position asked of the system
//! (`platypus_platform::cursor_in_window`, else the last known pointer).

use std::path::Path;

use platypus_platform::InputEvent;
use web_ui_dom::Input;

use super::{App, Surface};

/// A system drag over the window: what it carries and what happened this frame.
#[derive(Debug, Default)]
pub struct FileDrag {
    hovering: Vec<String>,
    /// Whether the UI has been told (`FileDragEnter`).
    entered: bool,
    dropped: Vec<String>,
    cancelled: bool,
    /// The last position sent, to send moves only when it changes.
    at: Option<(f32, f32)>,
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl FileDrag {
    /// Takes the file events; returns whether `event` was one.
    pub fn event(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::FileHovered(path) => {
                // A new hover after a drop or cancel starts over.
                if self.cancelled {
                    *self = Self::default();
                }
                self.hovering.push(path_text(path));
            }
            InputEvent::FileDropped(path) => self.dropped.push(path_text(path)),
            InputEvent::FileHoverCancelled => self.cancelled = true,
            _ => return false,
        }
        true
    }

    /// Whether anything is going on (the app then follows the cursor).
    pub fn busy(&self) -> bool {
        !self.hovering.is_empty() || !self.dropped.is_empty() || self.cancelled
    }

    /// The UI inputs for this frame, with the cursor at `cursor` (UI pixels).
    pub fn inputs(&mut self, cursor: Option<(f32, f32)>) -> Vec<Input> {
        let mut out = Vec::new();
        let at = cursor.or(self.at).unwrap_or_default();
        if !self.dropped.is_empty() {
            if self.entered && Some(at) != self.at {
                out.push(Input::PointerMove { x: at.0, y: at.1 });
            } else if !self.entered {
                // Dropped without a hover first: the drop happens where the pointer is.
                out.push(Input::PointerMove { x: at.0, y: at.1 });
            }
            out.push(Input::FileDrop {
                files: std::mem::take(&mut self.dropped),
            });
            *self = Self::default();
            return out;
        }
        if self.cancelled {
            if self.entered {
                out.push(Input::FileDragCancel);
            }
            *self = Self::default();
            return out;
        }
        if self.hovering.is_empty() {
            return out;
        }
        if !self.entered {
            self.entered = true;
            out.push(Input::FileDragEnter {
                files: self.hovering.clone(),
                x: at.0,
                y: at.1,
            });
        } else if Some(at) != self.at {
            out.push(Input::PointerMove { x: at.0, y: at.1 });
        }
        self.at = Some(at);
        out
    }
}

impl App {
    /// Once a frame: a system drag's files go to the screen's UI as drag events.
    pub(super) fn pump_file_drag(&mut self) {
        if !self.file_drag.busy() {
            return;
        }
        let scale = f64::from(self.scale());
        let cursor = self
            .gfx
            .as_ref()
            .and_then(|g| g.window.as_ref())
            .and_then(|w| platypus_platform::cursor_in_window(w))
            .or(self.pointer_px.map(|(x, y)| (f64::from(x), f64::from(y))))
            .map(|(x, y)| ((x / scale) as f32, (y / scale) as f32));
        for input in self.file_drag.inputs(cursor) {
            if let Input::FileDrop { files } = &input {
                // The editor may copy what was dropped (and nothing else from outside).
                platypus_engine_api::editor::allow_sources(files);
            }
            self.handle_on(Surface::Screen, input);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn hover(d: &mut FileDrag, p: &str) {
        assert!(d.event(&InputEvent::FileHovered(PathBuf::from(p))));
    }

    // M5-DND-05
    #[test]
    fn system_file_drags_become_ui_drag_inputs_once_a_frame() {
        let mut d = FileDrag::default();
        assert!(!d.busy());
        hover(&mut d, "a.png");
        hover(&mut d, "b.png");
        assert_eq!(
            d.inputs(Some((10.0, 20.0))),
            [Input::FileDragEnter {
                files: vec!["a.png".into(), "b.png".into()],
                x: 10.0,
                y: 20.0
            }]
        );
        // Moves only when the cursor moved.
        assert_eq!(d.inputs(Some((10.0, 20.0))), []);
        assert_eq!(
            d.inputs(Some((15.0, 20.0))),
            [Input::PointerMove { x: 15.0, y: 20.0 }]
        );
        assert!(d.event(&InputEvent::FileDropped(PathBuf::from("a.png"))));
        assert!(d.event(&InputEvent::FileDropped(PathBuf::from("b.png"))));
        assert_eq!(
            d.inputs(None),
            [Input::FileDrop {
                files: vec!["a.png".into(), "b.png".into()]
            }]
        );
        assert!(!d.busy());

        // Hover, then leave: a cancel. Other events aren't ours.
        hover(&mut d, "c.txt");
        d.inputs(None);
        assert!(d.event(&InputEvent::FileHoverCancelled));
        assert_eq!(d.inputs(None), [Input::FileDragCancel]);
        assert!(!d.busy());
        assert!(!d.event(&InputEvent::MouseLeave));
    }
}
