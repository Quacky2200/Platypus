// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Absolute boxes that escape clips. In CSS an element's `overflow` clip and scrolling
//! reach an absolute descendant only when the element is its containing block or lies
//! between them in the containing-block chain: a dropdown in a static `overflow: hidden`
//! box, held by a positioned box further out, shows in full and doesn't scroll with it.
//! Painting walks the element tree, so drawing such a box closes the clips it escapes and
//! reopens the rest around it; hit testing and scrolling skip the same elements.

use std::ops::{Deref, DerefMut};

use web_ui_core::NodeId;

use super::{ROOT, UiTree};
use crate::draw::{DrawCmd, Rect};
use crate::layout::LayoutEngine;
use crate::text_style::ComputedText;
use crate::transform::TransformContext;

/// A clip, transform or layer the draw list has open: where its push command is, and for
/// an `overflow` clip, the element it belongs to (the one clip an absolute box can escape).
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    at: usize,
    owner: Option<NodeId>,
}

/// The draw list being built, with the clips, transforms and layers open at its end.
#[derive(Default)]
pub(super) struct DrawOut {
    pub cmds: Vec<DrawCmd>,
    trail: Vec<Frame>,
}

impl Deref for DrawOut {
    type Target = Vec<DrawCmd>;
    fn deref(&self) -> &Vec<DrawCmd> {
        &self.cmds
    }
}

impl DerefMut for DrawOut {
    fn deref_mut(&mut self) -> &mut Vec<DrawCmd> {
        &mut self.cmds
    }
}

impl DrawOut {
    /// Pushes `cmd` (a `PushClip`, `PushTransform` or `PushLayer`) and remembers it as open;
    /// `owner` names the element of an `overflow` clip.
    pub fn open(&mut self, cmd: DrawCmd, owner: Option<NodeId>) {
        self.cmds.push(cmd);
        self.trail.push(Frame {
            at: self.cmds.len() - 1,
            owner,
        });
    }

    /// Pushes `cmd` (the matching pop) and forgets the innermost open command.
    pub fn close(&mut self, cmd: DrawCmd) {
        self.cmds.push(cmd);
        self.trail.pop();
    }

    /// Pops the open commands from `first` in (innermost first), returning them.
    fn unwind(&mut self, first: usize) -> Vec<Frame> {
        let frames = self.trail.split_off(first);
        for f in frames.iter().rev() {
            let pop = match self.cmds[f.at] {
                DrawCmd::PushTransform(_) => DrawCmd::PopTransform,
                DrawCmd::PushLayer { .. } => DrawCmd::PopLayer,
                _ => DrawCmd::PopClip,
            };
            self.cmds.push(pop);
        }
        frames
    }

    /// Opens `frames` again (outermost first), with the same commands.
    fn rewind(&mut self, frames: impl IntoIterator<Item = Frame>) {
        for f in frames {
            let cmd = self.cmds[f.at].clone();
            self.open(cmd, f.owner);
        }
    }
}

impl<L: LayoutEngine> UiTree<L> {
    /// Records the absolute boxes placed in a containing block further up (box, block) and
    /// the elements between each and its block, whose clips and scrolling it escapes.
    pub(super) fn set_escapes(&mut self, escapes: Vec<(NodeId, NodeId)>) {
        self.escapes.clear();
        self.escape_paths.clear();
        for (id, block) in escapes {
            let mut at = self.parent(id);
            while let Some(p) = at.filter(|p| *p != block) {
                self.escape_paths.insert(p);
                at = self.parent(p);
            }
            self.escapes.insert(id, block);
        }
    }

    /// Whether `id` is an absolute box that the `overflow` clip and scrolling of its
    /// ancestor `ancestor` don't reach: its containing block is further up.
    pub(super) fn escapes_clip_of(&self, id: NodeId, ancestor: NodeId) -> bool {
        self.escapes
            .get(&id)
            .is_some_and(|block| *block != ancestor && self.is_inclusive_ancestor(*block, ancestor))
    }

    /// Whether some absolute box inside `id` escapes `id`'s clip, so the subtree has to be
    /// visited even where that clip hides the rest.
    pub(super) fn holds_escapes(&self, id: NodeId) -> bool {
        self.escape_paths.contains(&id)
    }

    /// The elements whose scrolling moves `target`, innermost first: its ancestors, less
    /// those between an absolute box and its containing block (they scroll its siblings,
    /// not it). Wheel, keyboard and scroll-into-view scrolling go up this chain.
    pub fn scroll_path(&self, target: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut at = Some(target);
        while let Some(id) = at.filter(|id| *id != ROOT && self.nodes.contains_key(id)) {
            path.push(id);
            at = match self.escapes.get(&id) {
                Some(block) => Some(*block),
                None => self.nodes[&id].parent,
            };
        }
        path
    }

    /// Draws `id` like `collect`. An absolute box that escapes open `overflow` clips is drawn
    /// with those clips closed (and the clips and transforms inside them reopened), then
    /// they are opened again for its later siblings. Clips outside an open layer stay: the
    /// layer is clipped as a whole when it is composited.
    pub(super) fn collect(
        &self,
        id: NodeId,
        inherited: &ComputedText,
        xf: TransformContext,
        cull: Option<Rect>,
        out: &mut DrawOut,
    ) {
        let first = match self.escapes.contains_key(&id) {
            true => self.first_escaped(id, out),
            false => None,
        };
        let Some(first) = first else {
            return self.collect_node(id, inherited, xf, cull, out);
        };
        let frames = out.unwind(first);
        let kept = frames
            .iter()
            .filter(|f| !f.owner.is_some_and(|o| self.escapes_clip_of(id, o)))
            .copied()
            .collect::<Vec<_>>();
        let base = out.trail.len();
        out.rewind(kept);
        // The culling area came from the clips it escapes.
        self.collect_node(id, inherited, xf, None, out);
        out.unwind(base);
        out.rewind(frames);
    }

    /// The first open frame `id` escapes, after the innermost open layer.
    fn first_escaped(&self, id: NodeId, out: &DrawOut) -> Option<usize> {
        let after_layer = out
            .trail
            .iter()
            .rposition(|f| matches!(out.cmds[f.at], DrawCmd::PushLayer { .. }))
            .map_or(0, |i| i + 1);
        (after_layer..out.trail.len()).find(|i| {
            out.trail[*i]
                .owner
                .is_some_and(|o| self.escapes_clip_of(id, o))
        })
    }
}
