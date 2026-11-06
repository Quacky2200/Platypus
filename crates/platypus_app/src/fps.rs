// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The FPS counter overlay: raw measured frame rate in the top right corner, drawn by the
//! app over everything (the world, the game's UI) for every game, whatever its backend.
//! The counting and text are plain functions; `FpsOverlay` is a tiny UI tree for the text.

use platypus_core::{NodeId, Value};
use web_ui_dom::opcodes::Op;
use web_ui_dom::{DrawCmd, ROOT, UiTree};

/// Frames are counted over windows of about this long (seconds), then shown.
pub const WINDOW: f64 = 0.25;

/// The text for `frames` drawn in `secs` seconds: an integer, `144 FPS`.
pub fn format_fps(frames: u32, secs: f64) -> String {
    let fps = if secs > 0.0 {
        f64::from(frames) / secs
    } else {
        0.0
    };
    format!("{} FPS", fps.round() as u64)
}

/// Counts frames over windows of [`WINDOW`] seconds of real time.
#[derive(Debug, Default)]
pub struct FpsCounter {
    start: f64,
    frames: u32,
    text: String,
    value: f64,
}

impl FpsCounter {
    /// The last full window's frame rate (unrounded); 0 before one closed.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// One frame was presented at `now` (seconds). Returns the new text when a window
    /// closed with this frame.
    pub fn frame(&mut self, now: f64) -> Option<&str> {
        self.frames += 1;
        let elapsed = now - self.start;
        if elapsed >= WINDOW {
            self.value = f64::from(self.frames) / elapsed;
            self.text = format_fps(self.frames, elapsed);
            self.start = now;
            self.frames = 0;
            return Some(&self.text);
        }
        None
    }

    /// The text to show at `now`: the last full window's, or (before any window closed,
    /// e.g. in a short screenshot run) the partial one.
    pub fn text(&self, now: f64) -> String {
        if self.text.is_empty() {
            format_fps(self.frames, now - self.start)
        } else {
            self.text.clone()
        }
    }
}

/// Whether the overlay is on. Precedence: the command-line flag (`--fps` / `--no-fps`),
/// then off by `PLATYPUS_FPS=0` or `"platypus": { "fps": false }`, then the default: on, except
/// in screenshot runs (so goldens and screenshots stay the same).
pub fn enabled(
    flag: Option<bool>,
    screenshot: bool,
    env: Option<&str>,
    manifest: Option<bool>,
) -> bool {
    if let Some(on) = flag {
        return on;
    }
    let env_off = env.is_some_and(|v| matches!(v.trim(), "0" | "false" | "off" | "no"));
    if env_off || manifest == Some(false) {
        return false;
    }
    !screenshot
}

/// The overlay's UI: one absolutely placed text, 10px from the top and right edges.
pub struct FpsOverlay {
    tree: UiTree,
    text: NodeId,
    shown: String,
}

impl FpsOverlay {
    pub fn new() -> Self {
        let mut tree = UiTree::with_system_fonts(1.0, 1.0);
        let (boxn, text) = (NodeId(1), NodeId(2));
        let style = |pairs: &[(&str, Value)]| Op::SetStyle {
            id: boxn,
            pairs: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        };
        let errors = tree.apply_ops(&[
            Op::Create {
                id: boxn,
                kind: "box".into(),
            },
            style(&[
                ("position", "absolute".into()),
                ("top", 10.0.into()),
                ("right", 10.0.into()),
                ("color", "#32ff32".into()),
                ("fontSize", 13.0.into()),
                ("fontWeight", 700.0.into()),
                ("textAlign", "right".into()),
                ("textShadow", "0 1px 2px #000".into()),
                ("pointerEvents", "none".into()),
            ]),
            Op::CreateText {
                id: text,
                text: String::new(),
            },
            Op::AppendChild {
                parent: boxn,
                child: text,
            },
            Op::AppendChild {
                parent: ROOT,
                child: boxn,
            },
        ]);
        for e in errors {
            tracing::warn!("fps overlay: {e}");
        }
        Self {
            tree,
            text,
            shown: String::new(),
        }
    }

    /// The draw list for a window of `width` x `height` physical pixels at `scale`.
    pub fn draw_list(&mut self, text: &str, width: u32, height: u32, scale: f32) -> Vec<DrawCmd> {
        if self.shown != text {
            self.shown = text.to_owned();
            let _ = self.tree.apply_ops(&[Op::SetText {
                id: self.text,
                text: text.to_owned(),
            }]);
        }
        self.tree
            .set_size(width as f32 / scale, height as f32 / scale);
        self.tree.draw_list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-API-900
    #[test]
    fn the_text_is_the_integer_rate() {
        assert_eq!(format_fps(36, 0.25), "144 FPS");
        assert_eq!(format_fps(15, 0.25), "60 FPS");
        assert_eq!(format_fps(7, 0.25), "28 FPS");
        assert_eq!(format_fps(3, 0.02), "150 FPS");
        assert_eq!(format_fps(1, 0.0), "0 FPS");
    }

    // M9-API-901
    #[test]
    fn rounding_goes_to_the_nearest_integer() {
        assert_eq!(format_fps(10, 0.0625), "160 FPS");
        assert_eq!(format_fps(37, 0.25), "148 FPS");
        assert_eq!(format_fps(1, 0.3), "3 FPS");
        assert_eq!(format_fps(1, 0.7), "1 FPS");
    }

    // M9-API-902
    #[test]
    fn a_window_closes_after_a_quarter_second() {
        let mut c = FpsCounter::default();
        // 100 FPS: frames every 10 ms.
        let mut shown = Vec::new();
        for i in 1..=60 {
            if let Some(t) = c.frame(i as f64 * 0.01) {
                shown.push(t.to_owned());
            }
        }
        // Windows close at 0.25 s and 0.50 s (the 25th and 50th frames).
        assert_eq!(shown.len(), 2);
        assert!(shown.iter().all(|t| t == "100 FPS"), "{shown:?}");
    }

    // M9-API-916
    #[test]
    fn the_last_windows_rate_is_kept_unrounded_for_scripts() {
        let mut c = FpsCounter::default();
        assert_eq!(c.value(), 0.0);
        // 3 frames in 0.3 s: 10 FPS, not yet a closed window for the first two.
        c.frame(0.1);
        c.frame(0.2);
        assert_eq!(c.value(), 0.0);
        c.frame(0.3);
        assert!((c.value() - 10.0).abs() < 1e-9, "{}", c.value());
    }

    // M9-API-903
    #[test]
    fn the_rate_is_not_smoothed_over_windows() {
        let mut c = FpsCounter::default();
        // A fast window (200 FPS), then a slow one (20 FPS).
        let mut now = 0.0;
        let mut last = String::new();
        while now < 0.25 {
            now += 0.005;
            if let Some(t) = c.frame(now) {
                last = t.to_owned();
            }
        }
        assert_eq!(last, "200 FPS");
        let end = now + 0.25;
        while now < end - 1e-9 {
            now += 0.05;
            if let Some(t) = c.frame(now) {
                last = t.to_owned();
            }
        }
        assert_eq!(last, "20 FPS");
    }

    // M9-API-904
    #[test]
    fn a_short_run_shows_the_partial_window() {
        let mut c = FpsCounter::default();
        for i in 1..=30 {
            assert!(c.frame(i as f64 * 0.001).is_none());
        }
        assert_eq!(c.text(0.03), "1000 FPS");
    }

    // M9-API-905
    #[test]
    fn the_overlay_is_on_by_default_and_off_for_screenshots() {
        assert!(enabled(None, false, None, None));
        assert!(!enabled(None, true, None, None));
        assert!(!enabled(None, true, Some("1"), Some(true)));
    }

    // M9-API-906
    #[test]
    fn env_and_manifest_turn_it_off() {
        assert!(!enabled(None, false, Some("0"), None));
        assert!(!enabled(None, false, Some("false"), None));
        assert!(enabled(None, false, Some("1"), None));
        assert!(!enabled(None, false, None, Some(false)));
        assert!(enabled(None, false, None, Some(true)));
    }

    // M9-API-907
    #[test]
    fn flags_override_everything() {
        assert!(enabled(Some(true), true, Some("0"), Some(false)));
        assert!(!enabled(Some(false), false, Some("1"), Some(true)));
    }

    // M9-API-908
    #[test]
    fn the_overlay_draws_text_in_the_top_right_corner() {
        let mut overlay = FpsOverlay::new();
        let cmds = overlay.draw_list("144 FPS", 800, 600, 1.0);
        assert!(!cmds.is_empty());
        let empty = overlay.draw_list("", 800, 600, 1.0);
        assert!(empty.len() < cmds.len() || empty != cmds);
    }
}
