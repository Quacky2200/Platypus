// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Bot AI for scripts: `NavGrid` (3D A* pathfinding, `platypus_ai::grid`) and `Steering`
//! (`platypus_ai::steering`, plain functions over positions and velocities). A `NavGrid` is a
//! handle to a grid kept here per thread; [`reset`] forgets them all (a reload).

use glam::{IVec3, Vec3};
use platypus_ai::steering::Agent;
use platypus_ai::{Connectivity, NavGrid};
use platypus_math::{V3, v3};
use std::cell::RefCell;
use ts_api::{Handle, ts_api};

/// A `NavGrid` by number (1-based: 0 is "none").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NavGridRef(pub u32);

impl Handle for NavGridRef {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        NavGridRef(raw as u32)
    }
}

thread_local! {
    static GRIDS: RefCell<Vec<Option<NavGrid>>> = const { RefCell::new(Vec::new()) };
}

/// Forgets every grid (a reload, or a test).
pub fn reset() {
    GRIDS.with(|g| g.borrow_mut().clear());
}

fn with_grid<R>(r: NavGridRef, f: impl FnOnce(&mut NavGrid) -> R) -> Option<R> {
    GRIDS.with(|g| {
        let mut g = g.borrow_mut();
        let i = (r.0 as usize).checked_sub(1)?;
        g.get_mut(i)?.as_mut().map(f)
    })
}

fn vec(v: V3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

fn v3_of(v: Vec3) -> V3 {
    v3(f64::from(v.x), f64::from(v.y), f64::from(v.z))
}

/// A script's cell coordinates (whole numbers in a `Vector3`).
fn cell(v: V3) -> IVec3 {
    IVec3::new(v.x.round() as i32, v.y.round() as i32, v.z.round() as i32)
}

fn cell_v3(c: IVec3) -> V3 {
    v3(f64::from(c.x), f64::from(c.y), f64::from(c.z))
}

#[ts_api(
    namespace = "NavGrid", like,
    handles(NavGrid = NavGridRef),
    values(Vector3 = V3 [x: f64, y: f64, z: f64])
)]
pub mod nav_grid_api {
    use super::*;

    /// A 3D grid of cells for pathfinding with A*. Cells are `Vector3`s of whole numbers; a cell
    /// is blocked or free and costs `weight` (at least 1) to enter. Paths run through cell centres
    /// in world space; `connectivity` 6 moves along faces, 18 adds edges, 26 (the default) adds corners.
    impl NavGridRef {
        /// `new NavGrid(size, origin, cellSize?)`: `size` cells per axis from the world point `origin`
        /// (the grid's minimum corner), each `cellSize` (default 1) metres wide.
        #[optional(cell_size = 1)]
        pub fn create(size: V3, origin: V3, cell_size: f64) -> NavGridRef {
            let g = NavGrid::new(cell(size), vec(origin), cell_size as f32);
            GRIDS.with(|all| {
                let mut all = all.borrow_mut();
                all.push(Some(g));
                NavGridRef(all.len() as u32)
            })
        }

        /// Forgets the grid (a handle is a number, nothing frees it by itself).
        pub fn destroy(self) {
            GRIDS.with(|all| {
                let mut all = all.borrow_mut();
                if let Some(slot) = (self.0 as usize)
                    .checked_sub(1)
                    .and_then(|i| all.get_mut(i))
                {
                    *slot = None;
                }
            });
        }

        /// Blocks or frees one cell. Cells outside the grid are always blocked.
        pub fn set_blocked(self, cell_at: V3, blocked: bool) {
            with_grid(self, |g| g.set_blocked(cell(cell_at), blocked));
        }

        /// Whether a cell is blocked (outside the grid counts as blocked).
        pub fn is_blocked(self, cell_at: V3) -> bool {
            with_grid(self, |g| g.is_blocked(cell(cell_at))).unwrap_or(true)
        }

        /// Blocks or frees every cell the world-space box touches (a wall, a crate).
        pub fn set_box_blocked(self, min: V3, max: V3, blocked: bool) {
            with_grid(self, |g| g.set_box_blocked(vec(min), vec(max), blocked));
        }

        /// What entering a cell costs, as a multiple of its distance (mud 3, fire 20); at least 1.
        pub fn set_weight(self, cell_at: V3, weight: f64) {
            with_grid(self, |g| g.set_weight(cell(cell_at), weight as f32));
        }

        /// 6, 18 or 26 neighbours per cell (anything else means 26).
        pub fn set_connectivity(self, neighbours: f64) {
            let c = match neighbours as i32 {
                6 => Connectivity::Six,
                18 => Connectivity::Eighteen,
                _ => Connectivity::TwentySix,
            };
            with_grid(self, |g| g.connectivity = c);
        }

        /// Whether diagonal steps may pass between two blocked cells touching their corner (off).
        pub fn set_corner_cutting(self, allowed: bool) {
            with_grid(self, |g| g.corner_cutting = allowed);
        }

        /// Gives up a search after this many expanded cells (no limit by default).
        pub fn set_max_expansions(self, count: f64) {
            with_grid(self, |g| {
                g.max_expansions = if count >= 1.0 {
                    count as usize
                } else {
                    usize::MAX
                };
            });
        }

        /// The cell a world point falls in.
        pub fn world_to_cell(self, p: V3) -> V3 {
            with_grid(self, |g| cell_v3(g.world_to_cell(vec(p)))).unwrap_or(V3::ZERO)
        }

        /// The world position of a cell's centre.
        pub fn cell_to_world(self, cell_at: V3) -> V3 {
            with_grid(self, |g| v3_of(g.cell_to_world(cell(cell_at)))).unwrap_or(V3::ZERO)
        }

        /// The cheapest route from `from` to `to` in world space, starting at `from` and ending at
        /// `to`; empty when an end is blocked or there is no route. `smooth` (default true) drops
        /// every point the one before it can already see past.
        #[optional(smooth = true)]
        pub fn find_path(self, from: V3, to: V3, smooth: bool) -> Vec<V3> {
            with_grid(self, |g| g.find_path(vec(from), vec(to), smooth))
                .flatten()
                .map(|p| p.into_iter().map(v3_of).collect())
                .unwrap_or_default()
        }

        /// Whether the straight line between two world points crosses only free cells.
        pub fn line_of_sight(self, a: V3, b: V3) -> bool {
            with_grid(self, |g| g.line_of_sight(vec(a), vec(b))).unwrap_or(false)
        }
    }
}

fn agent(position: V3, velocity: V3, max_speed: f64, max_force: f64) -> Agent {
    let mut a = Agent::new(vec(position), max_speed as f32, max_force as f32);
    a.velocity = vec(velocity);
    a
}

#[ts_api(
    namespace = "Steering", like,
    values(Vector3 = V3 [x: f64, y: f64, z: f64])
)]
pub mod steering_api {
    use super::*;

    // Every function returns an acceleration (a force on a unit mass), at most `maxForce` long:
    // add up the behaviours you want, then `velocity += accel * dt`, capped at `maxSpeed`.

    /// Runs flat out at `target`.
    pub fn seek(position: V3, velocity: V3, target: V3, max_speed: f64, max_force: f64) -> V3 {
        v3_of(agent(position, velocity, max_speed, max_force).seek(vec(target)))
    }

    /// Runs flat out away from `threat`.
    pub fn flee(position: V3, velocity: V3, threat: V3, max_speed: f64, max_force: f64) -> V3 {
        v3_of(agent(position, velocity, max_speed, max_force).flee(vec(threat)))
    }

    /// Seeks `target` and slows to a stop within `slowRadius` of it.
    pub fn arrive(
        position: V3,
        velocity: V3,
        target: V3,
        slow_radius: f64,
        max_speed: f64,
        max_force: f64,
    ) -> V3 {
        v3_of(
            agent(position, velocity, max_speed, max_force).arrive(vec(target), slow_radius as f32),
        )
    }

    /// Seeks where a moving target will be.
    pub fn pursue(
        position: V3,
        velocity: V3,
        target: V3,
        target_velocity: V3,
        max_speed: f64,
        max_force: f64,
    ) -> V3 {
        v3_of(
            agent(position, velocity, max_speed, max_force)
                .pursue(vec(target), vec(target_velocity)),
        )
    }

    /// Flees from where a moving threat will be.
    pub fn evade(
        position: V3,
        velocity: V3,
        threat: V3,
        threat_velocity: V3,
        max_speed: f64,
        max_force: f64,
    ) -> V3 {
        v3_of(
            agent(position, velocity, max_speed, max_force)
                .evade(vec(threat), vec(threat_velocity)),
        )
    }

    /// Pushes away from `neighbours` closer than `radius`, harder the closer they are.
    pub fn separation(
        position: V3,
        velocity: V3,
        neighbours: &[V3],
        radius: f64,
        max_speed: f64,
        max_force: f64,
    ) -> V3 {
        let pts: Vec<Vec3> = neighbours.iter().map(|&n| vec(n)).collect();
        v3_of(agent(position, velocity, max_speed, max_force).separation(&pts, radius as f32))
    }

    /// Wanders: seeks a point on a sphere of `radius` ahead by `distance`, in the direction
    /// `jitter` gives (keep it between frames and nudge it a little for smooth wandering).
    pub fn wander(
        position: V3,
        velocity: V3,
        distance: f64,
        radius: f64,
        jitter: V3,
        max_speed: f64,
        max_force: f64,
    ) -> V3 {
        v3_of(agent(position, velocity, max_speed, max_force).wander(
            distance as f32,
            radius as f32,
            vec(jitter),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // AI-20
    #[test]
    fn grid_handle_finds_a_path_around_a_wall_and_forgets_on_destroy() {
        reset();
        let g = NavGridRef::create(v3(10.0, 10.0, 1.0), v3(0.0, 0.0, 0.0), 1.0);
        g.set_box_blocked(v3(5.0, 0.0, 0.0), v3(5.4, 7.0, 0.9), true);
        let path = g.find_path(v3(1.5, 1.5, 0.5), v3(8.5, 1.5, 0.5), true);
        assert!(path.len() >= 3);
        assert_eq!(path[0], v3(1.5, 1.5, 0.5));
        assert_eq!(*path.last().unwrap(), v3(8.5, 1.5, 0.5));
        assert!(g.is_blocked(v3(5.0, 3.0, 0.0)));
        assert!(!g.line_of_sight(v3(1.5, 1.5, 0.5), v3(8.5, 1.5, 0.5)));
        g.destroy();
        assert!(
            g.find_path(v3(1.5, 1.5, 0.5), v3(8.5, 1.5, 0.5), true)
                .is_empty()
        );
    }

    // AI-21
    #[test]
    fn blocked_ends_and_sealed_goals_give_an_empty_path() {
        reset();
        let g = NavGridRef::create(v3(5.0, 5.0, 1.0), v3(0.0, 0.0, 0.0), 1.0);
        g.set_blocked(v3(4.0, 4.0, 0.0), true);
        assert!(
            g.find_path(v3(0.5, 0.5, 0.5), v3(4.5, 4.5, 0.5), true)
                .is_empty()
        );
        assert!(
            g.find_path(v3(0.5, 0.5, 0.5), v3(40.0, 0.5, 0.5), true)
                .is_empty()
        );
    }

    // AI-22
    #[test]
    fn steering_functions_head_the_right_way() {
        let (p, z) = (v3(0.0, 0.0, 0.0), V3::ZERO);
        assert!(steering_api::seek(p, z, v3(5.0, 0.0, 0.0), 4.0, 10.0).x > 0.0);
        assert!(steering_api::flee(p, z, v3(5.0, 0.0, 0.0), 4.0, 10.0).x < 0.0);
        let a = steering_api::arrive(p, z, v3(0.0, 0.0, 0.1), 3.0, 4.0, 10.0);
        assert!(a.z > 0.0 && a.magnitude() <= 10.0 + 1e-6);
        let near = [v3(0.5, 0.0, 0.0)];
        assert!(steering_api::separation(p, z, &near, 2.0, 4.0, 10.0).x < 0.0);
    }
}
