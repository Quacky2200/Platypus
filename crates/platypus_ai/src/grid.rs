// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A 3D navigation grid: blocked cells, per-cell costs, A* over it, line of sight and
//! path smoothing.

use crate::astar::{Graph, find_path_limited};
use glam::{IVec3, Vec3};

/// A cell's integer coordinates.
pub type Cell = IVec3;

/// Which neighbours a cell connects to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connectivity {
    /// Faces only.
    Six,
    /// Faces and edges.
    Eighteen,
    /// Faces, edges and corners.
    TwentySix,
}

impl Connectivity {
    fn allows(self, step: IVec3) -> bool {
        let n = step.abs().element_sum();
        match self {
            Connectivity::Six => n == 1,
            Connectivity::Eighteen => n <= 2,
            Connectivity::TwentySix => true,
        }
    }
}

/// Cells of `cell_size` metres from `origin` (the minimum corner), `size` cells per axis.
#[derive(Clone, Debug)]
pub struct NavGrid {
    size: IVec3,
    origin: Vec3,
    cell_size: f32,
    blocked: Vec<bool>,
    /// Cost multiplier per cell, at least 1 (keeps the heuristic admissible).
    weight: Vec<f32>,
    pub connectivity: Connectivity,
    /// Whether a diagonal step may pass between two blocked cells touching its corner.
    pub corner_cutting: bool,
    /// Expansion limit for one search.
    pub max_expansions: usize,
}

impl NavGrid {
    pub fn new(size: IVec3, origin: Vec3, cell_size: f32) -> Self {
        let size = size.max(IVec3::ONE);
        let n = (size.x * size.y * size.z) as usize;
        Self {
            size,
            origin,
            cell_size: cell_size.max(1e-6),
            blocked: vec![false; n],
            weight: vec![1.0; n],
            connectivity: Connectivity::TwentySix,
            corner_cutting: false,
            max_expansions: usize::MAX,
        }
    }

    pub fn size(&self) -> IVec3 {
        self.size
    }

    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    pub fn contains(&self, c: Cell) -> bool {
        c.cmpge(IVec3::ZERO).all() && c.cmplt(self.size).all()
    }

    fn index(&self, c: Cell) -> usize {
        ((c.z * self.size.y + c.y) * self.size.x + c.x) as usize
    }

    /// Outside the grid counts as blocked.
    pub fn is_blocked(&self, c: Cell) -> bool {
        !self.contains(c) || self.blocked[self.index(c)]
    }

    pub fn set_blocked(&mut self, c: Cell, blocked: bool) {
        if self.contains(c) {
            let i = self.index(c);
            self.blocked[i] = blocked;
        }
    }

    /// Blocks (or frees) every cell touching the world-space box.
    pub fn set_box_blocked(&mut self, min: Vec3, max: Vec3, blocked: bool) {
        let lo = self.world_to_cell(min.min(max));
        let hi = self.world_to_cell(min.max(max));
        for z in lo.z.max(0)..=hi.z.min(self.size.z - 1) {
            for y in lo.y.max(0)..=hi.y.min(self.size.y - 1) {
                for x in lo.x.max(0)..=hi.x.min(self.size.x - 1) {
                    self.set_blocked(IVec3::new(x, y, z), blocked);
                }
            }
        }
    }

    /// A cost multiplier for entering the cell (mud, danger), clamped to at least 1.
    pub fn set_weight(&mut self, c: Cell, weight: f32) {
        if self.contains(c) {
            let i = self.index(c);
            self.weight[i] = weight.max(1.0);
        }
    }

    pub fn weight(&self, c: Cell) -> f32 {
        if self.contains(c) {
            self.weight[self.index(c)]
        } else {
            f32::INFINITY
        }
    }

    pub fn world_to_cell(&self, p: Vec3) -> Cell {
        ((p - self.origin) / self.cell_size).floor().as_ivec3()
    }

    /// The cell's centre.
    pub fn cell_to_world(&self, c: Cell) -> Vec3 {
        self.origin + (c.as_vec3() + 0.5) * self.cell_size
    }

    /// The nearest free cell to `c` within `radius` cells, searching outward in rings.
    pub fn nearest_free(&self, c: Cell, radius: i32) -> Option<Cell> {
        if !self.is_blocked(c) {
            return Some(c);
        }
        let mut best: Option<(i32, Cell)> = None;
        for dz in -radius..=radius {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let n = c + IVec3::new(dx, dy, dz);
                    let d = dx * dx + dy * dy + dz * dz;
                    if !self.is_blocked(n) && best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, n));
                    }
                }
            }
        }
        best.map(|(_, n)| n)
    }

    /// Whether the step from `c` by `step` is open: the target is free and, without corner
    /// cutting, so is every cell the step brushes past.
    fn step_open(&self, c: Cell, step: IVec3) -> bool {
        if self.is_blocked(c + step) {
            return false;
        }
        if self.corner_cutting {
            return true;
        }
        let axes: Vec<IVec3> = [
            IVec3::new(step.x, 0, 0),
            IVec3::new(0, step.y, 0),
            IVec3::new(0, 0, step.z),
        ]
        .into_iter()
        .filter(|a| *a != IVec3::ZERO)
        .collect();
        if axes.len() < 2 {
            return true;
        }
        // Every proper non-empty subset of the step's axes must be free too.
        (1..(1u32 << axes.len()) - 1).all(|mask| {
            let part = axes
                .iter()
                .enumerate()
                .filter(|(i, _)| mask >> i & 1 == 1)
                .fold(IVec3::ZERO, |s, (_, a)| s + *a);
            !self.is_blocked(c + part)
        })
    }

    /// The cheapest cell path, centres included. `None` when either end is blocked or no
    /// route exists.
    pub fn find_cell_path(&self, start: Cell, goal: Cell) -> Option<Vec<Cell>> {
        if self.is_blocked(start) || self.is_blocked(goal) {
            return None;
        }
        find_path_limited(self, start, goal, self.max_expansions).map(|p| p.nodes)
    }

    /// A world-space path from `from` to `to`, running through cell centres, with the first
    /// and last points replaced by the exact ends. Smoothed by line of sight when `smooth`.
    pub fn find_path(&self, from: Vec3, to: Vec3, smooth: bool) -> Option<Vec<Vec3>> {
        let cells = self.find_cell_path(self.world_to_cell(from), self.world_to_cell(to))?;
        let mut pts: Vec<Vec3> = cells.iter().map(|&c| self.cell_to_world(c)).collect();
        if smooth {
            pts = self.smooth(&pts);
        }
        if let Some(first) = pts.first_mut() {
            *first = from;
        }
        if let Some(last) = pts.last_mut() {
            *last = to;
        }
        pts.dedup();
        Some(pts)
    }

    /// Whether the straight segment crosses only free cells (a 3D DDA walk, so it cannot
    /// slip through a corner).
    pub fn line_of_sight(&self, a: Vec3, b: Vec3) -> bool {
        let mut cell = self.world_to_cell(a);
        let end = self.world_to_cell(b);
        if self.is_blocked(cell) {
            return false;
        }
        let d = (b - a) / self.cell_size;
        let step = IVec3::new(sign(d.x), sign(d.y), sign(d.z));
        let rel = (a - self.origin) / self.cell_size;
        let mut t_max = Vec3::splat(f32::INFINITY);
        let mut t_delta = Vec3::splat(f32::INFINITY);
        for i in 0..3 {
            if d[i] != 0.0 {
                let next = if step[i] > 0 {
                    cell[i] as f32 + 1.0
                } else {
                    cell[i] as f32
                };
                t_max[i] = (next - rel[i]) / d[i];
                t_delta[i] = 1.0 / d[i].abs();
            }
        }
        // At most one cell change per axis step along the segment.
        let limit = (end - cell).abs().element_sum() + 1;
        for _ in 0..limit {
            if cell == end {
                return true;
            }
            let axis = if t_max.x <= t_max.y && t_max.x <= t_max.z {
                0
            } else if t_max.y <= t_max.z {
                1
            } else {
                2
            };
            if t_max[axis] > 1.0 {
                return true;
            }
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
            if self.is_blocked(cell) {
                return false;
            }
        }
        !self.is_blocked(end)
    }

    /// Drops every waypoint the one before it can already see past (string pulling).
    pub fn smooth(&self, pts: &[Vec3]) -> Vec<Vec3> {
        if pts.len() < 3 {
            return pts.to_vec();
        }
        let mut out = vec![pts[0]];
        let mut anchor = 0;
        let mut i = 2;
        while i < pts.len() {
            if !self.line_of_sight(pts[anchor], pts[i]) {
                anchor = i - 1;
                out.push(pts[anchor]);
            }
            i += 1;
        }
        out.push(pts[pts.len() - 1]);
        out
    }
}

fn sign(v: f32) -> i32 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

impl Graph for NavGrid {
    type Node = Cell;

    fn neighbors(&self, node: Cell, out: &mut Vec<(Cell, f32)>) {
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let step = IVec3::new(dx, dy, dz);
                    if step == IVec3::ZERO || !self.connectivity.allows(step) {
                        continue;
                    }
                    if self.step_open(node, step) {
                        let to = node + step;
                        let len = step.as_vec3().length() * self.cell_size;
                        out.push((to, len * self.weight(to)));
                    }
                }
            }
        }
    }

    fn heuristic(&self, from: Cell, to: Cell) -> f32 {
        (to - from).as_vec3().length() * self.cell_size
    }
}
