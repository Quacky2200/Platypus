// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A full-window message: a black screen with red centred text. `platypus-app` shows it instead of
//! the UI when a game asks for the 3D world, which its GLES renderer doesn't draw.

use platypus_core::{NodeId, Value};
use web_ui_dom::opcodes::Op;
use web_ui_dom::{DrawCmd, ROOT, UiTree};

pub struct Notice {
    tree: UiTree,
}

impl Notice {
    pub fn new(message: &str) -> Self {
        let mut tree = UiTree::with_system_fonts(1.0, 1.0);
        let (boxn, text) = (NodeId(1), NodeId(2));
        let pairs: &[(&str, Value)] = &[
            ("position", "absolute".into()),
            ("top", 0.0.into()),
            ("left", 0.0.into()),
            ("right", 0.0.into()),
            ("bottom", 0.0.into()),
            ("display", "flex".into()),
            ("alignItems", "center".into()),
            ("justifyContent", "center".into()),
            ("backgroundColor", "#000".into()),
            ("color", "#ff3030".into()),
            ("fontSize", 28.0.into()),
            ("fontWeight", 700.0.into()),
            ("textAlign", "center".into()),
        ];
        let errors = tree.apply_ops(&[
            Op::Create {
                id: boxn,
                kind: "box".into(),
            },
            Op::SetStyle {
                id: boxn,
                pairs: pairs
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            },
            Op::CreateText {
                id: text,
                text: message.to_owned(),
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
            tracing::warn!("notice: {e}");
        }
        Self { tree }
    }

    /// The draw list for a window of `width` x `height` physical pixels at `scale`.
    pub fn draw_list(&mut self, width: u32, height: u32, scale: f32) -> Vec<DrawCmd> {
        self.tree
            .set_size(width as f32 / scale, height as f32 / scale);
        self.tree.draw_list()
    }
}
