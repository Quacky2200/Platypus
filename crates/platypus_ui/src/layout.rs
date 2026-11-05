// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use rustc_hash::FxHashMap as HashMap;

use taffy::detailed_info::DetailedGridInfo;
use taffy::{
    AvailableSpace, BlockContext, Cache, CacheTree, Clear, ClearState, DetailedLayoutInfo, Display,
    Layout, LayoutBlockContainer, LayoutContainingBlock, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, ResolveOrZero, RoundTree,
    RunMode, Size, Style, TraversePartialTree, TraverseTree, compute_block_layout,
    compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_hidden_layout,
    compute_leaf_layout, compute_oof_layout, compute_root_layout, round_layout,
};
use web_ui_core::NodeId;

use crate::text::{AvailableWidth, Band};

/// What floats beside a measured leaf: lines of text avoid them.
#[derive(Clone, Copy, Debug)]
pub struct Around<'a> {
    /// The room left in the content box (empty: all of it).
    pub bands: &'a [Band],
    /// Whether this is the final layout of the leaf, not a sizing query.
    pub layout_pass: bool,
}

/// Sizes a measured leaf (text, fields, images): given the known content width and
/// height, if any, the available width and the floats beside it, returns its content
/// size (width, height).
pub type Measure<'a> =
    dyn FnMut(NodeId, (Option<f32>, Option<f32>), AvailableWidth, Around<'_>) -> (f32, f32) + 'a;

/// A node's computed layout, relative to its parent's border box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Resolved border widths: top, right, bottom, left.
    pub border: [f32; 4],
    /// Resolved padding: top, right, bottom, left.
    pub padding: [f32; 4],
    /// How far the content can scroll (x, y): its reachable extent beyond the padding box.
    pub scroll_max: [f32; 2],
}

/// The layout backend (DESIGN.md §5.3). Taffy is the default; a Yoga backend can
/// implement this and run the same fixtures (`tests/layout/*.json`).
///
/// Taffy's `Style` doubles as the backend-neutral description of CSS layout
/// properties; another backend translates it.
pub trait LayoutEngine {
    fn create_node(&mut self, id: NodeId, style: &Style);
    fn remove_node(&mut self, id: NodeId);
    fn set_style(&mut self, id: NodeId, style: &Style);
    fn set_children(&mut self, parent: NodeId, children: &[NodeId]);
    /// Marks a leaf whose size comes from `compute`'s `measure` callback (text).
    fn set_measured(&mut self, id: NodeId, measured: bool);
    /// Marks a measured leaf as a block of text: in a block container it lays out as a
    /// block (beside floats) whatever its own `display`.
    fn set_text_block(&mut self, id: NodeId, text_block: bool);
    /// Marks a node for relayout without a style change (e.g. its text changed).
    fn mark_dirty(&mut self, id: NodeId);
    /// Lays out the tree under `root` within `width` × `height` logical pixels,
    /// calling `measure` for measured leaves.
    fn compute(&mut self, root: NodeId, width: f32, height: f32, measure: &mut Measure<'_>);
    fn layout(&self, id: NodeId) -> Option<NodeLayout>;
    /// For an out-of-flow (absolute) box whose containing block is not its parent (a
    /// positioned ancestor further up, or the root): that box. Its [`NodeLayout`] position is
    /// then relative to the containing block's border box, not the parent's.
    fn containing_block(&self, id: NodeId) -> Option<NodeId>;
}

/// [`LayoutEngine`] backed by Taffy's algorithms over Platypus's own node storage.
///
/// Taffy's ready-made `TaffyTree` can't evaluate `calc()` (its `resolve_calc_value`
/// returns 0), so this implements Taffy's low-level tree traits itself, mirroring
/// `TaffyTree`, and resolves calc handles through [`crate::calc`]. Layout results
/// are cached per node, so only subtrees whose styles or children changed are
/// recomputed.
pub struct TaffyLayout {
    nodes: HashMap<u64, NodeData>,
    /// How many nodes set `float` or `clear`. While any does, a block's layout depends on
    /// the floats before it, which Taffy's per-node cache doesn't see, so blocks inside a
    /// formatting context skip the cache.
    floats: usize,
}

/// `style` as a block container sees a text block: flex and grid (the default `display`
/// of every element) mean nothing to a leaf, and would make it avoid floats like a new
/// formatting context instead of wrapping its lines around them.
fn as_block(style: &Style) -> Style {
    let mut block = style.clone();
    if matches!(block.display, Display::Flex | Display::Grid) {
        block.display = Display::Block;
    }
    block
}

fn uses_floats(style: &Style) -> bool {
    style.float != taffy::Float::None || style.clear != taffy::Clear::None
}

struct NodeData {
    style: Style,
    children: Vec<taffy::NodeId>,
    parent: Option<taffy::NodeId>,
    /// A leaf sized by the `measure` callback (text, text fields).
    measured: bool,
    /// The style a text block shows its block container: its own with `display: block`.
    /// `None` for other nodes.
    block_style: Option<Style>,
    unrounded: Layout,
    final_layout: Layout,
    /// Out-of-flow boxes whose containing block is this node.
    hoisted: Vec<taffy::NodeId>,
    /// The containing block whose `hoisted` list holds this node, so removing the node
    /// takes it off that list too (a cached layout would otherwise still visit it).
    hoisted_by: Option<taffy::NodeId>,
    cache: Cache,
    /// Size queries kept beyond Taffy's cache: exact inputs and their results. Taffy
    /// keeps nine and drops one for each new query, and intrinsic sizing through a few
    /// nested containers (a scroller in a row, say) asks a node more ways than that, so
    /// each pass would measure whole subtrees again.
    sizes: Vec<(LayoutInput, LayoutOutput)>,
    detailed: DetailedLayoutInfo,
}

/// Most size queries kept per node in [`NodeData::sizes`].
const MAX_SIZES: usize = 32;

impl NodeData {
    fn cache_get(&mut self, input: &LayoutInput) -> Option<LayoutOutput> {
        self.cache.get(input).or_else(|| {
            let found = self.sizes.iter().find(|(i, _)| i == input);
            found.map(|(_, output)| output.clone())
        })
    }

    fn cache_store(&mut self, input: &LayoutInput, output: LayoutOutput) {
        if input.run_mode == RunMode::ComputeSize && !self.sizes.iter().any(|(i, _)| i == input) {
            if self.sizes.len() == MAX_SIZES {
                self.sizes.remove(0);
            }
            self.sizes.push((*input, output.clone()));
        }
        self.cache.store(input, output);
    }

    /// Clears both caches; false if they were already empty.
    fn clear_caches(&mut self) -> bool {
        let sizes = !self.sizes.is_empty();
        self.sizes.clear();
        !matches!(self.cache.clear(), ClearState::AlreadyEmpty) | sizes
    }

    fn new(style: Style) -> Self {
        Self {
            style,
            children: Vec::new(),
            parent: None,
            measured: false,
            block_style: None,
            unrounded: Layout::new(),
            final_layout: Layout::new(),
            hoisted: Vec::new(),
            hoisted_by: None,
            cache: Cache::new(),
            sizes: Vec::new(),
            detailed: DetailedLayoutInfo::None,
        }
    }
}

fn key(id: NodeId) -> taffy::NodeId {
    taffy::NodeId::from(u64::from(id.0))
}

impl TaffyLayout {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::default(),
            floats: 0,
        }
    }

    fn node(&self, id: taffy::NodeId) -> &NodeData {
        &self.nodes[&u64::from(id)]
    }

    fn node_mut(&mut self, id: taffy::NodeId) -> &mut NodeData {
        self.nodes.get_mut(&u64::from(id)).expect("node exists")
    }

    /// Clears the cached layout of `id` and its ancestors (stopping at one that was
    /// already clear, as `TaffyTree` does).
    fn mark_dirty_up(&mut self, id: taffy::NodeId) {
        let mut at = Some(id);
        while let Some(n) = at {
            let Some(node) = self.nodes.get_mut(&u64::from(n)) else {
                return;
            };
            if !node.clear_caches() {
                return;
            }
            at = node.parent;
        }
    }
}

impl Default for TaffyLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine for TaffyLayout {
    fn create_node(&mut self, id: NodeId, style: &Style) {
        self.remove_node(id);
        self.floats += usize::from(uses_floats(style));
        self.nodes
            .insert(u64::from(id.0), NodeData::new(style.clone()));
    }

    fn remove_node(&mut self, id: NodeId) {
        let Some(node) = self.nodes.remove(&u64::from(id.0)) else {
            return;
        };
        self.floats -= usize::from(uses_floats(&node.style));
        let k = key(id);
        if let Some(parent) = node.parent
            && let Some(p) = self.nodes.get_mut(&u64::from(parent))
        {
            p.children.retain(|c| *c != k);
            self.mark_dirty_up(parent);
        }
        for child in node.children {
            if let Some(c) = self.nodes.get_mut(&u64::from(child))
                && c.parent == Some(k)
            {
                c.parent = None;
            }
        }
        // Off its containing block's out-of-flow list, and its own list's boxes free.
        if let Some(owner) = node.hoisted_by
            && let Some(o) = self.nodes.get_mut(&u64::from(owner))
        {
            o.hoisted.retain(|h| *h != k);
            self.mark_dirty_up(owner);
        }
        for h in node.hoisted {
            if let Some(n) = self.nodes.get_mut(&u64::from(h))
                && n.hoisted_by == Some(k)
            {
                n.hoisted_by = None;
            }
        }
    }

    fn set_style(&mut self, id: NodeId, style: &Style) {
        let k = key(id);
        if let Some(node) = self.nodes.get_mut(&u64::from(k)) {
            let before = uses_floats(&node.style);
            node.style = style.clone();
            if node.block_style.is_some() {
                node.block_style = Some(as_block(style));
            }
            self.floats = self.floats + usize::from(uses_floats(style)) - usize::from(before);
            self.mark_dirty_up(k);
        }
    }

    fn set_children(&mut self, parent: NodeId, children: &[NodeId]) {
        let pk = key(parent);
        if !self.nodes.contains_key(&u64::from(pk)) {
            return;
        }
        let new: Vec<taffy::NodeId> = children
            .iter()
            .map(|c| key(*c))
            .filter(|c| self.nodes.contains_key(&u64::from(*c)))
            .collect();
        let old = std::mem::replace(&mut self.node_mut(pk).children, new.clone());
        for c in old {
            if let Some(n) = self.nodes.get_mut(&u64::from(c))
                && n.parent == Some(pk)
            {
                n.parent = None;
            }
        }
        for c in new {
            // A child moved from another parent leaves that parent's list too.
            let previous = self.node(c).parent;
            if let Some(prev) = previous.filter(|p| *p != pk)
                && let Some(p) = self.nodes.get_mut(&u64::from(prev))
            {
                p.children.retain(|x| *x != c);
                self.mark_dirty_up(prev);
            }
            self.node_mut(c).parent = Some(pk);
        }
        self.mark_dirty_up(pk);
    }

    fn set_measured(&mut self, id: NodeId, measured: bool) {
        let k = key(id);
        if let Some(node) = self.nodes.get_mut(&u64::from(k))
            && node.measured != measured
        {
            node.measured = measured;
            self.mark_dirty_up(k);
        }
    }

    fn set_text_block(&mut self, id: NodeId, text_block: bool) {
        let k = key(id);
        if let Some(node) = self.nodes.get_mut(&u64::from(k))
            && node.block_style.is_some() != text_block
        {
            node.block_style = text_block.then(|| as_block(&node.style));
            self.mark_dirty_up(k);
        }
    }

    fn mark_dirty(&mut self, id: NodeId) {
        self.mark_dirty_up(key(id));
    }

    fn compute(&mut self, root: NodeId, width: f32, height: f32, measure: &mut Measure<'_>) {
        let root = key(root);
        if !self.nodes.contains_key(&u64::from(root)) {
            return;
        }
        let available = Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        };
        let mut view = View {
            layout: self,
            measure,
        };
        compute_root_layout(&mut view, root, available);
        round_layout(&mut view, root);
    }

    fn layout(&self, id: NodeId) -> Option<NodeLayout> {
        let l = &self.nodes.get(&u64::from(id.0))?.final_layout;
        Some(NodeLayout {
            x: l.location.x,
            y: l.location.y,
            width: l.size.width,
            height: l.size.height,
            border: [l.border.top, l.border.right, l.border.bottom, l.border.left],
            padding: [
                l.padding.top,
                l.padding.right,
                l.padding.bottom,
                l.padding.left,
            ],
            scroll_max: [l.scroll_width(), l.scroll_height()],
        })
    }

    fn containing_block(&self, id: NodeId) -> Option<NodeId> {
        let node = self.nodes.get(&u64::from(id.0))?;
        if !node.style.position.is_out_of_flow() {
            return None;
        }
        let block = node.hoisted_by.filter(|b| Some(*b) != node.parent)?;
        Some(NodeId(u64::from(block) as u32))
    }
}

/// A layout pass over [`TaffyLayout`]: the tree plus the measure callback for leaves.
/// Implements Taffy's tree traits like `TaffyTree`'s own view does.
struct View<'a, 'm> {
    layout: &'a mut TaffyLayout,
    measure: &'a mut Measure<'m>,
}

impl View<'_, '_> {
    fn style(&self, id: taffy::NodeId) -> &Style {
        &self.layout.node(id).style
    }

    fn compute_child(
        &mut self,
        node_id: taffy::NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node_id);
        }
        // A block in a formatting context lays out against the floats placed before it,
        // which the cache's key doesn't include.
        let uncached = self.layout.floats > 0 && block_ctx.is_some();
        let compute = |tree: &mut Self, node_id: taffy::NodeId, inputs: LayoutInput| {
            let display = tree.style(node_id).display;
            let has_children = tree.child_count(node_id) > 0;
            let mut output = match (display, has_children) {
                (Display::None, _) => compute_hidden_layout(tree, node_id),
                (Display::Block, true) => compute_block_layout(tree, node_id, inputs, block_ctx),
                (Display::FlowRoot, true) => compute_block_layout(tree, node_id, inputs, None),
                (Display::Flex, true) => compute_flexbox_layout(tree, node_id, inputs),
                (Display::Grid, true) => compute_grid_layout(tree, node_id, inputs),
                (_, false) => {
                    let bands = block_ctx
                        .as_deref()
                        .map(|ctx| tree.float_bands(node_id, inputs, ctx))
                        .unwrap_or_default();
                    tree.measure_leaf(node_id, inputs, &bands)
                }
            };
            if inputs.run_mode == RunMode::PerformLayout {
                compute_oof_layout(tree, node_id, &mut output);
            }
            output
        };
        if uncached {
            compute(self, node_id, inputs)
        } else {
            compute_cached_layout(self, node_id, inputs, compute)
        }
    }

    /// The room floats leave in the content box of the leaf `node_id` (a text block
    /// laid out in the formatting context `ctx`), as bands from its top; empty when
    /// nothing is beside it.
    fn float_bands(
        &self,
        node_id: taffy::NodeId,
        inputs: LayoutInput,
        ctx: &BlockContext<'_>,
    ) -> Vec<Band> {
        if !self.layout.node(node_id).measured || !ctx.has_floats() {
            return Vec::new();
        }
        let style = self.style(node_id);
        let basis = inputs.parent_size.width;
        let pad = style.padding.resolve_or_zero(basis, crate::calc::resolve);
        let border = style.border.resolve_or_zero(basis, crate::calc::resolve);
        let (left, top) = (pad.left + border.left, pad.top + border.top);
        // Far below every float: the whole width.
        let full = ctx.find_content_slot(1.0e9, Clear::None, None);
        let content_width = (full.width - left - pad.right - border.right).max(0.0);
        let mut bands: Vec<Band> = Vec::new();
        let (mut after, mut at) = (None, 0.0);
        // Each float segment is a band down to the next one.
        loop {
            let slot = ctx.find_content_slot(at, Clear::None, after);
            let Some(segment) = slot.segment_id else {
                break;
            };
            let x = (slot.x - left).max(0.0);
            let right = (slot.x + slot.width - left).min(content_width);
            let y = (slot.y - top).max(0.0);
            let band = Band {
                y,
                x,
                width: (right - x).max(0.0),
            };
            match bands.last_mut() {
                Some(last) if last.y >= y => *last = band,
                _ => bands.push(band),
            }
            (after, at) = (Some(segment), slot.y);
        }
        // Below the lowest float the width is free again.
        if let Some(end) = ctx.cleared_threshold(Clear::Both) {
            let y = (end - top).max(0.0);
            if bands.last().is_some_and(|b| b.y < y) {
                bands.push(Band {
                    y,
                    x: 0.0,
                    width: content_width,
                });
            }
        }
        if bands.first().is_some_and(|b| b.y > 0.0) {
            bands.insert(
                0,
                Band {
                    y: 0.0,
                    x: 0.0,
                    width: content_width,
                },
            );
        }
        let free = |b: &Band| b.x < 0.01 && b.width > content_width - 0.01;
        if bands.iter().all(free) {
            bands.clear();
        }
        bands
    }

    /// A leaf: Taffy handles padding and borders; `measure` sizes the content of
    /// measured leaves (text), others have none. `bands` are the floats beside it.
    fn measure_leaf(
        &mut self,
        node_id: taffy::NodeId,
        inputs: LayoutInput,
        bands: &[Band],
    ) -> LayoutOutput {
        let node = self.layout.node(node_id);
        let measured = node.measured.then_some(NodeId(u64::from(node_id) as u32));
        let around = Around {
            bands,
            layout_pass: inputs.run_mode == RunMode::PerformLayout,
        };
        let measure = &mut *self.measure;
        compute_leaf_layout(
            inputs,
            &node.style,
            crate::calc::resolve,
            |known, available| {
                let Some(id) = measured else {
                    return Size::ZERO;
                };
                let width = match available.width {
                    AvailableSpace::Definite(w) => AvailableWidth::Definite(w),
                    AvailableSpace::MinContent => AvailableWidth::MinContent,
                    AvailableSpace::MaxContent => AvailableWidth::MaxContent,
                };
                let (w, h) = measure(id, (known.width, known.height), width, around);
                Size {
                    width: known.width.unwrap_or(w),
                    height: known.height.unwrap_or(h),
                }
            },
        )
    }
}

impl TraversePartialTree for View<'_, '_> {
    type ChildIter<'a>
        = std::iter::Copied<std::slice::Iter<'a, taffy::NodeId>>
    where
        Self: 'a;

    fn child_ids(&self, parent: taffy::NodeId) -> Self::ChildIter<'_> {
        self.layout.node(parent).children.iter().copied()
    }

    fn child_count(&self, parent: taffy::NodeId) -> usize {
        self.layout.node(parent).children.len()
    }

    fn get_child_id(&self, parent: taffy::NodeId, index: usize) -> taffy::NodeId {
        self.layout.node(parent).children[index]
    }
}

impl TraverseTree for View<'_, '_> {}

impl LayoutPartialTree for View<'_, '_> {
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type CustomIdent = String;

    fn get_core_container_style(&self, node_id: taffy::NodeId) -> Self::CoreContainerStyle<'_> {
        self.style(node_id)
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &Layout) {
        self.layout.node_mut(node_id).unrounded = *layout;
    }

    fn resolve_calc_value(&self, val: *const (), basis: f32) -> f32 {
        crate::calc::resolve(val, basis)
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: LayoutInput,
    ) -> LayoutOutput {
        self.compute_child(node_id, inputs, None)
    }
}

impl LayoutContainingBlock for View<'_, '_> {
    type OofItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_oof_item_style(&self, node_id: taffy::NodeId) -> Self::OofItemStyle<'_> {
        self.style(node_id)
    }

    fn clear_hoisted_children(&mut self, node_id: taffy::NodeId) {
        let old = std::mem::take(&mut self.layout.node_mut(node_id).hoisted);
        for h in old {
            if let Some(n) = self.layout.nodes.get_mut(&u64::from(h))
                && n.hoisted_by == Some(node_id)
            {
                n.hoisted_by = None;
            }
        }
    }

    fn add_hoisted_children(&mut self, node_id: taffy::NodeId, hoisted: &[taffy::NodeId]) {
        self.layout
            .node_mut(node_id)
            .hoisted
            .extend_from_slice(hoisted);
        for h in hoisted {
            if let Some(n) = self.layout.nodes.get_mut(&u64::from(*h)) {
                n.hoisted_by = Some(node_id);
            }
        }
    }

    fn get_detailed_layout_info(&self, node_id: taffy::NodeId) -> &DetailedLayoutInfo {
        &self.layout.node(node_id).detailed
    }
}

impl CacheTree for View<'_, '_> {
    fn cache_get(&mut self, node_id: taffy::NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.layout.node_mut(node_id).cache_get(input)
    }

    fn cache_store(&mut self, node_id: taffy::NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.layout.node_mut(node_id).cache_store(input, output)
    }

    fn cache_clear(&mut self, node_id: taffy::NodeId) {
        self.layout.node_mut(node_id).clear_caches();
    }
}

impl LayoutBlockContainer for View<'_, '_> {
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: taffy::NodeId) -> Self::BlockContainerStyle<'_> {
        self.style(node_id)
    }

    fn get_block_child_style(&self, child: taffy::NodeId) -> Self::BlockItemStyle<'_> {
        let node = self.layout.node(child);
        node.block_style.as_ref().unwrap_or(&node.style)
    }

    fn compute_block_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_child(node_id, inputs, block_ctx)
    }
}

impl LayoutFlexboxContainer for View<'_, '_> {
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_flexbox_container_style(
        &self,
        node_id: taffy::NodeId,
    ) -> Self::FlexboxContainerStyle<'_> {
        self.style(node_id)
    }

    fn get_flexbox_child_style(&self, child: taffy::NodeId) -> Self::FlexboxItemStyle<'_> {
        self.style(child)
    }
}

impl LayoutGridContainer for View<'_, '_> {
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        self.style(node_id)
    }

    fn get_grid_child_style(&self, child: taffy::NodeId) -> Self::GridItemStyle<'_> {
        self.style(child)
    }

    fn set_detailed_grid_info(&mut self, node_id: taffy::NodeId, info: DetailedGridInfo) {
        self.layout.node_mut(node_id).detailed = DetailedLayoutInfo::Grid(Box::new(info));
    }
}

impl RoundTree for View<'_, '_> {
    fn get_unrounded_layout(&self, node_id: taffy::NodeId) -> Layout {
        self.layout.node(node_id).unrounded
    }

    fn set_final_layout(&mut self, node_id: taffy::NodeId, layout: &Layout) {
        self.layout.node_mut(node_id).final_layout = *layout;
    }

    fn is_out_of_flow(&self, node_id: taffy::NodeId) -> bool {
        let style = self.style(node_id);
        style.position.is_out_of_flow() && style.display != Display::None
    }

    fn hoisted_child_count(&self, node_id: taffy::NodeId) -> usize {
        self.layout.node(node_id).hoisted.len()
    }

    fn get_hoisted_child_id(&self, node_id: taffy::NodeId, index: usize) -> taffy::NodeId {
        self.layout.node(node_id).hoisted[index]
    }
}
