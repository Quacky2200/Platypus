// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Shared helpers for building small trees in integration tests.
#![allow(dead_code)]

use platypus_ui::UiTree;
use platypus_ui::opcodes::*;
use web_ui_core::{Commit, Value};

pub fn n(id: u32) -> Value {
    Value::Number(id.into())
}

pub fn op(code: u32, args: &[Value]) -> Value {
    let mut v = vec![Value::Number(code.into())];
    v.extend_from_slice(args);
    Value::Array(v)
}

pub struct Dom {
    pub ops: Vec<Value>,
    pub next: u32,
}

impl Dom {
    pub fn new() -> Self {
        Self {
            ops: Vec::new(),
            next: 1,
        }
    }

    /// A box with inline styles, appended to `parent`.
    pub fn add(&mut self, parent: u32, kind: &str, style: &[(&str, Value)]) -> u32 {
        let id = self.next;
        self.next += 1;
        self.ops.push(op(CREATE, &[n(id), kind.into()]));
        if !style.is_empty() {
            let mut a = vec![n(id)];
            for (k, v) in style {
                a.push((*k).into());
                a.push(v.clone());
            }
            self.ops.push(op(SET_STYLE, &a));
        }
        self.ops.push(op(APPEND_CHILD, &[n(parent), n(id)]));
        id
    }

    pub fn text(&mut self, parent: u32, text: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        self.ops.push(op(CREATE_TEXT, &[n(id), text.into()]));
        self.ops.push(op(APPEND_CHILD, &[n(parent), n(id)]));
        id
    }

    pub fn set_class(&mut self, id: u32, class: &str) {
        self.ops
            .push(op(SET_ATTRS, &[n(id), "className".into(), class.into()]));
    }

    pub fn build(self, css: &str) -> UiTree {
        let mut tree = UiTree::new(400.0, 400.0);
        let errors = tree.apply_commit(&Commit { ops: self.ops });
        assert!(errors.is_empty(), "{errors:?}");
        if !css.is_empty() {
            let added = tree.add_stylesheet(css);
            assert!(added.warnings.is_empty(), "{:?}", added.warnings);
        }
        tree.layout_if_needed();
        tree
    }
}

pub fn px(v: f64) -> Value {
    Value::Number(v)
}
