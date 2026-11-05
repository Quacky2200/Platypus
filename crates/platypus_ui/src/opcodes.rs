// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Opcodes for UI commits, shared with `@platypus/react` (`js/react/src/opcodes.ts`
//! must match). Each op is an array `[opcode, ...args]`; node IDs are numbers.
//!
//! The root container is node [`crate::ROOT`] (`0`); the reconciler's own nodes
//! start at `1`. Removing a node also destroys its whole subtree: React never
//! reinserts a removed instance (moves use `APPEND_CHILD`/`INSERT_BEFORE` directly).

use crate::tree::UiError;
use web_ui_core::{NodeId, Value};

/// `[CREATE, id, type]`: creates a detached node of element `type` (e.g. `"box"`).
pub const CREATE: u32 = 0;

/// `[SET_STYLE, id, prop, value, prop, value, ...]`: sets style properties.
/// A `null` value resets the property to its default.
pub const SET_STYLE: u32 = 1;

/// `[APPEND_CHILD, parent, child]`: appends (or moves) `child` to the end of `parent`.
pub const APPEND_CHILD: u32 = 2;

/// `[INSERT_BEFORE, parent, child, before]`: inserts (or moves) `child` before `before`.
pub const INSERT_BEFORE: u32 = 3;

/// `[REMOVE_CHILD, parent, child]`: detaches `child` and destroys its subtree.
pub const REMOVE_CHILD: u32 = 4;

/// `[CREATE_TEXT, id, text]`: creates a text node (a run of characters, element
/// type `#text`). Inside `text`/`span` elements it is inline content; elsewhere it
/// is laid out as its own anonymous text block.
pub const CREATE_TEXT: u32 = 5;

/// `[SET_TEXT, id, text]`: replaces a text node's characters.
pub const SET_TEXT: u32 = 6;

/// `[SET_ATTRS, id, name, value, name, value, ...]`: element attributes used by
/// CSS selectors: `className`, `id`, `disabled`, `checked`, `tabIndex` and `data-*`.
/// A `null` value removes the attribute.
pub const SET_ATTRS: u32 = 7;

/// One decoded op. `apply_commit` decodes the script's `[opcode, ...args]` arrays into these
/// (checking arity and argument types once, here); the native reconciler builds them directly.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Create {
        id: NodeId,
        kind: String,
    },
    SetStyle {
        id: NodeId,
        pairs: Vec<(String, Value)>,
    },
    AppendChild {
        parent: NodeId,
        child: NodeId,
    },
    InsertBefore {
        parent: NodeId,
        child: NodeId,
        before: NodeId,
    },
    RemoveChild {
        parent: NodeId,
        child: NodeId,
    },
    CreateText {
        id: NodeId,
        text: String,
    },
    SetText {
        id: NodeId,
        text: String,
    },
    SetAttrs {
        id: NodeId,
        pairs: Vec<(String, Value)>,
    },
}

impl Op {
    /// Decodes `[opcode, ...args]`.
    pub fn decode(op: &Value) -> Result<Op, UiError> {
        let Value::Array(args) = op else {
            return Err(UiError(format!("op must be an array, got {op:?}")));
        };
        let opcode = args
            .first()
            .and_then(Value::as_f64)
            .ok_or_else(|| UiError(format!("op is missing its opcode: {op:?}")))?;
        let arg = |i: usize| {
            args.get(i)
                .ok_or_else(|| UiError(format!("op is missing argument {i}: {op:?}")))
        };
        let id = |i: usize| -> Result<NodeId, UiError> {
            let v = arg(i)?;
            match v.as_f64() {
                Some(n) if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) => {
                    Ok(NodeId(n as u32))
                }
                _ => Err(UiError(format!("invalid node id {v:?}"))),
            }
        };
        let string = |i: usize, what: &str| -> Result<String, UiError> {
            Ok(arg(i)?
                .as_str()
                .ok_or_else(|| UiError(format!("{what}: {op:?}")))?
                .to_owned())
        };
        let pairs = |what: &str, names: &str| -> Result<Vec<(String, Value)>, UiError> {
            let rest = &args[2.min(args.len())..];
            if rest.len() % 2 != 0 {
                return Err(UiError(format!("{what} needs {names} pairs: {op:?}")));
            }
            rest.chunks(2)
                .map(|p| match p[0].as_str() {
                    Some(name) => Ok((name.to_owned(), p[1].clone())),
                    None => Err(UiError(format!(
                        "{what}: name must be a string: {:?}",
                        p[0]
                    ))),
                })
                .collect()
        };
        Ok(match opcode as u32 {
            CREATE => Op::Create {
                id: id(1)?,
                kind: string(2, "CREATE needs an element type")?,
            },
            CREATE_TEXT => Op::CreateText {
                id: id(1)?,
                text: string(2, "CREATE_TEXT needs a string")?,
            },
            SET_TEXT => Op::SetText {
                id: id(1)?,
                text: string(2, "SET_TEXT needs a string")?,
            },
            SET_STYLE => {
                let id = id(1)?;
                Op::SetStyle {
                    id,
                    pairs: pairs("SET_STYLE", "prop/value")?,
                }
            }
            SET_ATTRS => {
                let id = id(1)?;
                Op::SetAttrs {
                    id,
                    pairs: pairs("SET_ATTRS", "name/value")?,
                }
            }
            APPEND_CHILD => Op::AppendChild {
                parent: id(1)?,
                child: id(2)?,
            },
            INSERT_BEFORE => Op::InsertBefore {
                parent: id(1)?,
                child: id(2)?,
                before: id(3)?,
            },
            REMOVE_CHILD => Op::RemoveChild {
                parent: id(1)?,
                child: id(2)?,
            },
            other => return Err(UiError(format!("unknown opcode {other}"))),
        })
    }
}
