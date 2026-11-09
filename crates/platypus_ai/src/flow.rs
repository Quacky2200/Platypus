// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Flow fields: crowd navigation for any number of agents (rats, zombies, a stampede). One
//! search from the goals outward fills every cell of a [`NavGrid`] with its distance to the nearest
//! goal and the direction that way; each agent then just reads the field where it stands. A
//! thousand agents cost one search, not a thousand.
//!
//! [`Crowd`] moves a swarm along a field with local separation (so they spread through gaps
//! instead of piling up) and slides them along walls.

use crate::astar::Graph;
use crate::grid::{Cell, NavGrid};
use glam::{IVec3, Vec3};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Distances and directions to the nearest goal for every cell of a grid.
#[derive(Clone, Debug)]
pub struct FlowField {
    size: IVec3,
    origin: Vec3,
    cell: f32,
    /// Cost to the nearest goal (`f32::INFINITY`: blocked or cut off).
    dist: Vec<f32>,
    /// Unit direction of steepest descent (zero at goals and unreachable cells).
    dir: Vec<Vec3>,
}

struct Item {
    d: f32,
    i: usize,
}
impl PartialEq for Item {
    fn eq(&self, o: &Self) -> bool {
        self.d.total_cmp(&o.d).is_eq()
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    // Reversed: the heap pops the largest, we want the smallest distance.
    fn cmp(&self, o: &Self) -> Ordering {
        o.d.total_cmp(&self.d)
    }
}

impl FlowField {
    /// The field of `grid` towards `goals` (blocked goal cells are ignored). Costs follow the
    /// grid's own: connectivity, corner cutting and weights.
    pub fn build(grid: &NavGrid, goals: &[Cell]) -> Self {
        let size = grid.size();
        let cell = grid.cell_size();
        let origin = grid.cell_to_world(IVec3::ZERO) - Vec3::splat(cell * 0.5);
        let n = (size.x * size.y * size.z) as usize;
        let mut f = FlowField {
            size,
            origin,
            cell,
            dist: vec![f32::INFINITY; n],
            dir: vec![Vec3::ZERO; n],
        };

        let mut heap = BinaryHeap::new();
        for &g in goals {
            if !grid.is_blocked(g) {
                let i = f.index(g);
                f.dist[i] = 0.0;
                heap.push(Item { d: 0.0, i });
            }
        }
        let mut edges = Vec::new();
        while let Some(Item { d, i }) = heap.pop() {
            if d > f.dist[i] {
                continue;
            }
            let c = f.cell_of(i);
            edges.clear();
            grid.neighbors(c, &mut edges);
            // Stepping from a neighbour onto `c` costs what entering `c` costs.
            let enter = grid.weight(c);
            for &(m, _) in &edges {
                let len = (m - c).as_vec3().length() * cell;
                let nd = d + len * enter;
                let mi = f.index(m);
                if nd < f.dist[mi] {
                    f.dist[mi] = nd;
                    heap.push(Item { d: nd, i: mi });
                }
            }
        }

        // The direction of each cell: downhill, blended over every neighbour that is lower.
        for i in 0..n {
            let di = f.dist[i];
            if !di.is_finite() || di == 0.0 {
                continue;
            }
            let c = f.cell_of(i);
            edges.clear();
            grid.neighbors(c, &mut edges);
            let mut acc = Vec3::ZERO;
            for &(m, _) in &edges {
                let dm = f.dist[f.index(m)];
                if dm < di {
                    acc += (di - dm) * (m - c).as_vec3().normalize();
                }
            }
            f.dir[i] = acc.normalize_or_zero();
        }
        f
    }

    fn index(&self, c: Cell) -> usize {
        ((c.z * self.size.y + c.y) * self.size.x + c.x) as usize
    }

    fn cell_of(&self, i: usize) -> Cell {
        let i = i as i32;
        IVec3::new(
            i % self.size.x,
            (i / self.size.x) % self.size.y,
            i / (self.size.x * self.size.y),
        )
    }

    pub fn contains(&self, c: Cell) -> bool {
        c.cmpge(IVec3::ZERO).all() && c.cmplt(self.size).all()
    }

    pub fn world_to_cell(&self, p: Vec3) -> Cell {
        ((p - self.origin) / self.cell).floor().as_ivec3()
    }

    /// Cost to the nearest goal from a cell: infinite when blocked, cut off or outside.
    pub fn distance_at_cell(&self, c: Cell) -> f32 {
        if self.contains(c) {
            self.dist[self.index(c)]
        } else {
            f32::INFINITY
        }
    }

    /// Cost to the nearest goal from the cell containing `p`.
    pub fn distance(&self, p: Vec3) -> f32 {
        self.distance_at_cell(self.world_to_cell(p))
    }

    /// The direction of a cell (unit length, or zero at a goal and where there is no way).
    pub fn direction_at_cell(&self, c: Cell) -> Vec3 {
        if self.contains(c) {
            self.dir[self.index(c)]
        } else {
            Vec3::ZERO
        }
    }

    /// The way to go from `p`: the cell directions around it blended by distance, so an agent
    /// turns smoothly instead of in steps. At most unit length; shorter right at a goal (the cells
    /// there point nowhere), so it fades out as the goal is reached; zero where there is no way.
    pub fn direction(&self, p: Vec3) -> Vec3 {
        let q = (p - self.origin) / self.cell - Vec3::splat(0.5);
        let base = q.floor().as_ivec3();
        let f = q - base.as_vec3();
        let mut acc = Vec3::ZERO;
        let mut wsum = 0.0;
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let c = base + IVec3::new(dx, dy, dz);
                    if !self.contains(c) || !self.dist[self.index(c)].is_finite() {
                        continue;
                    }
                    let w = (if dx == 1 { f.x } else { 1.0 - f.x })
                        * (if dy == 1 { f.y } else { 1.0 - f.y })
                        * (if dz == 1 { f.z } else { 1.0 - f.z });
                    acc += self.dir[self.index(c)] * w;
                    wsum += w;
                }
            }
        }
        if wsum > 1e-6 { acc / wsum } else { Vec3::ZERO }
    }
}

/// A swarm moved along a [`FlowField`].
#[derive(Clone, Debug)]
pub struct Crowd {
    pub positions: Vec<Vec3>,
    pub velocities: Vec<Vec3>,
    /// Top speed of each agent (metres per second).
    pub speeds: Vec<f32>,
    /// Agents closer than twice this push each other apart.
    pub radius: f32,
    /// How hard separation pushes, as a fraction of the agent's speed.
    pub separation: f32,
    /// How quickly velocity follows the wanted one (per second).
    pub turn_rate: f32,
    /// An agent this close to the goal (by field distance) stops.
    pub goal_radius: f32,
}

impl Crowd {
    pub fn new(radius: f32) -> Self {
        Crowd {
            positions: Vec::new(),
            velocities: Vec::new(),
            speeds: Vec::new(),
            radius: radius.max(1e-3),
            separation: 1.0,
            turn_rate: 8.0,
            goal_radius: 0.0,
        }
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// Adds an agent; returns its index.
    pub fn add(&mut self, position: Vec3, speed: f32) -> usize {
        self.positions.push(position);
        self.velocities.push(Vec3::ZERO);
        self.speeds.push(speed);
        self.positions.len() - 1
    }

    /// Removes an agent. The last agent takes its index.
    pub fn remove(&mut self, i: usize) {
        if i < self.positions.len() {
            self.positions.swap_remove(i);
            self.velocities.swap_remove(i);
            self.speeds.swap_remove(i);
        }
    }

    /// Moves every agent `dt` seconds along `field`, staying out of the cells `grid` blocks.
    pub fn step(&mut self, field: &FlowField, grid: &NavGrid, dt: f32) {
        let n = self.positions.len();
        let reach = self.radius * 2.0;
        // A hash of the agents by cell of the size of the separation distance.
        let key = |p: Vec3| (p / reach).floor().as_ivec3();
        let mut hash: HashMap<IVec3, Vec<u32>> = HashMap::new();
        for (i, &p) in self.positions.iter().enumerate() {
            hash.entry(key(p)).or_default().push(i as u32);
        }

        let mut next_v = vec![Vec3::ZERO; n];
        for (i, out) in next_v.iter_mut().enumerate() {
            let p = self.positions[i];
            let speed = self.speeds[i];
            let mut want = if field.distance(p) <= self.goal_radius {
                Vec3::ZERO
            } else {
                field.direction(p) * speed
            };
            // Push away from neighbours inside the separation distance.
            let mut push = Vec3::ZERO;
            let k = key(p);
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let Some(cell) = hash.get(&(k + IVec3::new(dx, dy, dz))) else {
                            continue;
                        };
                        for &j in cell {
                            let j = j as usize;
                            if j == i {
                                continue;
                            }
                            let away = p - self.positions[j];
                            let d = away.length();
                            if d > 1e-5 && d < reach {
                                push += away / d * (1.0 - d / reach);
                            }
                        }
                    }
                }
            }
            want += push.clamp_length_max(1.0) * speed * self.separation;
            want = want.clamp_length_max(speed);
            let blend = 1.0 - (-self.turn_rate * dt).exp();
            *out = self.velocities[i].lerp(want, blend);
        }

        for (i, v) in next_v.into_iter().enumerate() {
            let p = self.positions[i];
            let mut np = p + v * dt;
            let mut v = v;
            if grid.is_blocked(grid.world_to_cell(np)) {
                // Slide: keep whichever axes can still move.
                let try_x = Vec3::new(np.x, p.y, p.z);
                let try_z = Vec3::new(p.x, p.y, np.z);
                if !grid.is_blocked(grid.world_to_cell(try_x)) {
                    np = try_x;
                    v.z = 0.0;
                } else if !grid.is_blocked(grid.world_to_cell(try_z)) {
                    np = try_z;
                    v.x = 0.0;
                } else {
                    np = p;
                    v = Vec3::ZERO;
                }
            }
            self.positions[i] = np;
            self.velocities[i] = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Connectivity;

    /// A one-cell-thick floor of `w` x `d` cells, 1 m each.
    fn floor(w: i32, d: i32) -> NavGrid {
        NavGrid::new(IVec3::new(w, 1, d), Vec3::ZERO, 1.0)
    }

    // AI-40
    #[test]
    fn distances_grow_away_from_the_goal_and_blocked_cells_are_cut_off() {
        let mut g = floor(8, 8);
        g.connectivity = Connectivity::Six;
        g.set_blocked(IVec3::new(5, 0, 5), true);
        let f = FlowField::build(&g, &[IVec3::new(0, 0, 0)]);
        assert_eq!(f.distance_at_cell(IVec3::new(0, 0, 0)), 0.0);
        assert!((f.distance_at_cell(IVec3::new(3, 0, 2)) - 5.0).abs() < 1e-4);
        assert!(f.distance_at_cell(IVec3::new(5, 0, 5)).is_infinite());
        assert!(f.distance_at_cell(IVec3::new(99, 0, 0)).is_infinite());
    }

    // AI-41
    #[test]
    fn sealed_off_areas_have_no_distance_and_no_direction() {
        let mut g = floor(9, 5);
        for z in 0..5 {
            g.set_blocked(IVec3::new(4, 0, z), true);
        }
        let f = FlowField::build(&g, &[IVec3::new(0, 0, 2)]);
        assert!(f.distance_at_cell(IVec3::new(8, 0, 2)).is_infinite());
        assert_eq!(f.direction_at_cell(IVec3::new(8, 0, 2)), Vec3::ZERO);
        assert_eq!(f.direction(Vec3::new(8.5, 0.5, 2.5)), Vec3::ZERO);
    }

    // AI-42
    #[test]
    fn following_the_field_goes_around_a_wall_to_the_goal() {
        let mut g = floor(12, 12);
        // A wall across x = 6 with a gap at z = 10.
        for z in 0..10 {
            g.set_blocked(IVec3::new(6, 0, z), true);
        }
        let goal = IVec3::new(10, 0, 1);
        let f = FlowField::build(&g, &[goal]);
        let mut p = Vec3::new(1.5, 0.5, 1.5);
        for _ in 0..2000 {
            assert!(!g.is_blocked(g.world_to_cell(p)));
            let d = f.direction(p);
            if d == Vec3::ZERO {
                break;
            }
            p += d * 0.05;
            if f.distance(p) < 0.6 {
                break;
            }
        }
        assert!(f.distance(p) < 1.0, "ended at {p:?}");
    }

    // AI-43
    #[test]
    fn direction_is_smooth_and_never_longer_than_one() {
        let g = floor(10, 10);
        let f = FlowField::build(&g, &[IVec3::new(9, 0, 9)]);
        let mut last = f.direction(Vec3::new(0.5, 0.5, 0.5));
        for k in 1..40 {
            let p = Vec3::new(0.5 + k as f32 * 0.1, 0.5, 0.5 + k as f32 * 0.07);
            let d = f.direction(p);
            assert!(d.length() <= 1.0 + 1e-4);
            // No jump between samples a tenth of a cell apart.
            assert!((d - last).length() < 0.35, "jump at {p:?}");
            last = d;
        }
    }

    // AI-44
    #[test]
    fn weights_bend_the_flow_around_costly_cells() {
        let mut g = floor(9, 5);
        g.connectivity = Connectivity::Six;
        for x in 2..7 {
            g.set_weight(IVec3::new(x, 0, 2), 20.0);
        }
        let f = FlowField::build(&g, &[IVec3::new(8, 0, 2)]);
        // From (1, 2) the way round the mud costs less than straight through it.
        assert!(f.distance_at_cell(IVec3::new(1, 0, 2)) < 40.0);
        let d = f.direction_at_cell(IVec3::new(3, 0, 1));
        assert!(d.x > 0.0);
    }

    // AI-45
    #[test]
    fn a_crowd_reaches_the_goal_without_entering_walls() {
        let mut g = floor(24, 16);
        for z in 0..11 {
            g.set_blocked(IVec3::new(12, 0, z), true);
        }
        let goal = IVec3::new(21, 0, 3);
        let f = FlowField::build(&g, &[goal]);
        let mut crowd = Crowd::new(0.25);
        // Room for the whole crowd: 150 agents of radius 0.25 need about 30 square metres.
        crowd.goal_radius = 3.0;
        for i in 0..150 {
            let x = 1.0 + (i % 10) as f32 * 0.5;
            let z = 1.0 + (i / 10) as f32 * 0.5;
            crowd.add(Vec3::new(x, 0.5, z), 3.0);
        }
        for _ in 0..1500 {
            crowd.step(&f, &g, 1.0 / 30.0);
            assert!(
                crowd
                    .positions
                    .iter()
                    .all(|&p| !g.is_blocked(g.world_to_cell(p)))
            );
        }
        let arrived = crowd
            .positions
            .iter()
            .filter(|&&p| f.distance(p) <= 4.5)
            .count();
        assert!(arrived >= 140, "only {arrived} of 150 arrived");
    }

    // AI-46
    #[test]
    fn separation_keeps_a_crowd_spread_out() {
        let g = floor(20, 20);
        let f = FlowField::build(&g, &[IVec3::new(19, 0, 10)]);
        let mut crowd = Crowd::new(0.3);
        crowd.goal_radius = 3.0;
        // Everyone starts in nearly the same spot.
        for i in 0..40 {
            crowd.add(
                Vec3::new(
                    2.0 + (i % 5) as f32 * 0.01,
                    0.5,
                    10.0 + (i / 5) as f32 * 0.01,
                ),
                3.0,
            );
        }
        for _ in 0..600 {
            crowd.step(&f, &g, 1.0 / 30.0);
        }
        let mut closest = f32::INFINITY;
        for i in 0..crowd.len() {
            for j in (i + 1)..crowd.len() {
                closest = closest.min(crowd.positions[i].distance(crowd.positions[j]));
            }
        }
        assert!(closest > 0.15, "two agents ended {closest} apart");
    }

    // AI-47
    #[test]
    fn removing_an_agent_keeps_the_rest_in_step() {
        let mut crowd = Crowd::new(0.3);
        crowd.add(Vec3::new(1.0, 0.0, 0.0), 1.0);
        crowd.add(Vec3::new(2.0, 0.0, 0.0), 2.0);
        crowd.add(Vec3::new(3.0, 0.0, 0.0), 3.0);
        crowd.remove(0);
        assert_eq!(crowd.len(), 2);
        assert_eq!(crowd.positions[0].x, 3.0);
        assert_eq!(crowd.speeds[0], 3.0);
        crowd.remove(9);
        assert_eq!(crowd.len(), 2);
    }
}
