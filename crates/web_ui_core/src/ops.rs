// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use crate::Value;

/// One command queued by a script for the engine (create a UI node, spawn an entity, ...).
///
/// M1 keeps ops generic (a name plus arguments). Hot paths will move to a binary
/// encoding once the op set settles (see TESTS.md M2-BEN-04).
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub name: String,
    pub args: Vec<Value>,
}

/// Ops collected from scripts, drained by the engine once per frame or commit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpBuffer {
    ops: Vec<Op>,
}

impl OpBuffer {
    pub fn push(&mut self, op: Op) {
        self.ops.push(op);
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Op> {
        self.ops.iter()
    }

    /// Takes every queued op, leaving the buffer empty.
    pub fn take(&mut self) -> OpBuffer {
        std::mem::take(self)
    }
}

impl IntoIterator for OpBuffer {
    type Item = Op;
    type IntoIter = std::vec::IntoIter<Op>;

    fn into_iter(self) -> Self::IntoIter {
        self.ops.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_preserves_order_and_empties_buffer() {
        let mut buf = OpBuffer::default();
        for i in 0..3u32 {
            buf.push(Op {
                name: "n".into(),
                args: vec![i.into()],
            });
        }
        let taken = buf.take();
        assert!(buf.is_empty());
        let order: Vec<_> = taken
            .iter()
            .map(|op| op.args[0].as_f64().unwrap())
            .collect();
        assert_eq!(order, [0.0, 1.0, 2.0]);
    }
}

/// One UI commit sent by the React reconciler with `__platypus_commit(ops)`.
///
/// Each op is a `Value::Array` of `[opcode, ...args]`; `platypus_ui` decodes them.
/// A commit is applied as a unit so the engine never sees a half-updated tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commit {
    pub ops: Vec<Value>,
}
