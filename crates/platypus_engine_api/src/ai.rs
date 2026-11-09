// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Bot AI for scripts: `NavGrid` (3D A* pathfinding, `platypus_ai::grid`) and `Steering`
//! (`platypus_ai::steering`, plain functions over positions and velocities). A `NavGrid` is a
//! handle to a grid kept here per thread; [`reset`] forgets them all (a reload).

use glam::{IVec3, Vec3};
use platypus_ai::steering::Agent;
use platypus_ai::{Connectivity, Crowd, FlowField, Goap, NavGrid};
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
    PLANNERS.with(|g| g.borrow_mut().clear());
    FIELDS.with(|g| g.borrow_mut().clear());
    CROWDS.with(|g| g.borrow_mut().clear());
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

/// A `Goap` planner by number (1-based: 0 is "none").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GoapRef(pub u32);

impl Handle for GoapRef {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        GoapRef(raw as u32)
    }
}

thread_local! {
    static PLANNERS: RefCell<Vec<Option<Goap>>> = const { RefCell::new(Vec::new()) };
}

fn with_planner<R>(r: GoapRef, f: impl FnOnce(&mut Goap) -> R) -> Option<R> {
    PLANNERS.with(|g| {
        let mut g = g.borrow_mut();
        let i = (r.0 as usize).checked_sub(1)?;
        g.get_mut(i)?.as_mut().map(f)
    })
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[ts_api(namespace = "Goap", handles(Goap = GoapRef))]
pub mod goap_api {
    use super::*;

    /// Goal-oriented action planning. Facts are named booleans (up to 64); a literal is a fact name,
    /// or `"!name"` for false, and a fact a list doesn't mention is false. An action has a cost,
    /// preconditions and effects (all literals); `plan` finds the cheapest list of actions that takes
    /// the world from its facts to ones meeting the goal.
    impl GoapRef {
        /// `new Goap()`: a planner with no actions.
        pub fn create() -> GoapRef {
            PLANNERS.with(|all| {
                let mut all = all.borrow_mut();
                all.push(Some(Goap::new()));
                GoapRef(all.len() as u32)
            })
        }

        /// Forgets the planner (a handle is a number, nothing frees it by itself).
        pub fn destroy(self) {
            PLANNERS.with(|all| {
                let mut all = all.borrow_mut();
                if let Some(slot) = (self.0 as usize)
                    .checked_sub(1)
                    .and_then(|i| all.get_mut(i))
                {
                    *slot = None;
                }
            });
        }

        /// Adds an action; false when its literals contradict each other or there are more than 64 facts.
        pub fn add_action(
            self,
            name: String,
            cost: f64,
            preconditions: &[String],
            effects: &[String],
        ) -> bool {
            with_planner(self, |g| {
                g.add_action(&name, cost as f32, &strs(preconditions), &strs(effects))
                    .is_ok()
            })
            .unwrap_or(false)
        }

        /// Gives up a plan after this many expanded states (no limit by default).
        pub fn set_max_expansions(self, count: f64) {
            with_planner(self, |g| {
                g.max_expansions = (count >= 1.0).then_some(count as usize);
            });
        }

        /// The names of the actions to run, in order, from the facts that are true now to a state
        /// meeting every goal literal. Empty when the goal is already met, and also when there is no
        /// plan (check `canPlan`).
        pub fn plan(self, facts: &[String], goal: &[String]) -> Vec<String> {
            with_planner(self, |g| g.plan_names(&strs(facts), &strs(goal)))
                .and_then(Result::ok)
                .flatten()
                .unwrap_or_default()
        }

        /// Whether any plan exists.
        pub fn can_plan(self, facts: &[String], goal: &[String]) -> bool {
            with_planner(self, |g| g.plan_names(&strs(facts), &strs(goal)))
                .and_then(Result::ok)
                .flatten()
                .is_some()
        }
    }
}

/// A `FlowField` by number (1-based: 0 is "none").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FlowFieldRef(pub u32);

impl Handle for FlowFieldRef {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        FlowFieldRef(raw as u32)
    }
}

/// A `Crowd` by number (1-based: 0 is "none").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CrowdRef(pub u32);

impl Handle for CrowdRef {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        CrowdRef(raw as u32)
    }
}

struct FieldEntry {
    field: FlowField,
    /// The grid it was built from (`rebuild` reads it again, so changes to it show).
    grid: NavGridRef,
}

struct CrowdEntry {
    crowd: Crowd,
    grid: NavGridRef,
    field: FlowFieldRef,
}

thread_local! {
    static FIELDS: RefCell<Vec<Option<FieldEntry>>> = const { RefCell::new(Vec::new()) };
    static CROWDS: RefCell<Vec<Option<CrowdEntry>>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f` on a grid without needing it mutable (so it can be borrowed next to other state).
fn read_grid<R>(r: NavGridRef, f: impl FnOnce(&NavGrid) -> R) -> Option<R> {
    GRIDS.with(|g| {
        let g = g.borrow();
        let i = (r.0 as usize).checked_sub(1)?;
        g.get(i)?.as_ref().map(f)
    })
}

fn build_field(grid: NavGridRef, goals: &[V3]) -> Option<FlowField> {
    read_grid(grid, |g| {
        let cells: Vec<IVec3> = goals.iter().map(|&p| g.world_to_cell(vec(p))).collect();
        FlowField::build(g, &cells)
    })
}

fn with_crowd<R>(r: CrowdRef, f: impl FnOnce(&mut CrowdEntry) -> R) -> Option<R> {
    CROWDS.with(|c| {
        let mut c = c.borrow_mut();
        let i = (r.0 as usize).checked_sub(1)?;
        c.get_mut(i)?.as_mut().map(f)
    })
}

#[ts_api(
    namespace = "FlowField", like,
    handles(FlowField = FlowFieldRef, NavGrid = NavGridRef),
    values(Vector3 = V3 [x: f64, y: f64, z: f64])
)]
pub mod flow_field_api {
    use super::*;

    /// Crowd navigation: one search from the goals outward gives every cell of a `NavGrid` its distance
    /// to the nearest goal and the direction that way. Any number of agents then read the field where
    /// they stand, so a thousand rats cost one search. `rebuild` after the grid or the goals change.
    impl FlowFieldRef {
        /// `new FlowField(grid, goals)`: the field of `grid` towards the world points `goals`
        /// (goals inside walls are ignored).
        pub fn create(grid: NavGridRef, goals: &[V3]) -> FlowFieldRef {
            let Some(field) = build_field(grid, goals) else {
                return FlowFieldRef(0);
            };
            FIELDS.with(|all| {
                let mut all = all.borrow_mut();
                all.push(Some(FieldEntry { field, grid }));
                FlowFieldRef(all.len() as u32)
            })
        }

        /// Forgets the field (a handle is a number, nothing frees it by itself).
        pub fn destroy(self) {
            FIELDS.with(|all| {
                let mut all = all.borrow_mut();
                if let Some(slot) = (self.0 as usize)
                    .checked_sub(1)
                    .and_then(|i| all.get_mut(i))
                {
                    *slot = None;
                }
            });
        }

        /// Searches again from new goals, on the grid as it is now (walls and weights that changed
        /// show up).
        pub fn rebuild(self, goals: &[V3]) {
            let grid = FIELDS.with(|all| {
                let all = all.borrow();
                (self.0 as usize)
                    .checked_sub(1)
                    .and_then(|i| all.get(i)?.as_ref().map(|e| e.grid))
            });
            let Some(grid) = grid else { return };
            if let Some(field) = build_field(grid, goals) {
                FIELDS.with(|all| {
                    let mut all = all.borrow_mut();
                    if let Some(Some(e)) = (self.0 as usize)
                        .checked_sub(1)
                        .and_then(|i| all.get_mut(i))
                    {
                        e.field = field;
                    }
                });
            }
        }

        /// Which way to go from a world point: at most length 1, smooth between cells, shorter near a
        /// goal, zero where there is no way (a wall, or cut off from every goal).
        pub fn direction(self, p: V3) -> V3 {
            FIELDS
                .with(|all| {
                    let all = all.borrow();
                    let i = (self.0 as usize).checked_sub(1)?;
                    all.get(i)?.as_ref().map(|e| e.field.direction(vec(p)))
                })
                .map_or(V3::ZERO, v3_of)
        }

        /// The cost of the cheapest way from a world point to a goal in metres (weights counted);
        /// `Infinity` where there is none.
        pub fn distance(self, p: V3) -> f64 {
            FIELDS
                .with(|all| {
                    let all = all.borrow();
                    let i = (self.0 as usize).checked_sub(1)?;
                    all.get(i)?.as_ref().map(|e| e.field.distance(vec(p)))
                })
                .map_or(f64::INFINITY, f64::from)
        }
    }
}

#[ts_api(
    namespace = "Crowd", like,
    handles(Crowd = CrowdRef, NavGrid = NavGridRef, FlowField = FlowFieldRef),
    values(Vector3 = V3 [x: f64, y: f64, z: f64])
)]
pub mod crowd_api {
    use super::*;

    /// A swarm walking a `FlowField`: each agent follows the field with a little separation from the
    /// others, and slides along the grid's walls. Agents are numbered from 0; `remove` moves the last
    /// agent into the gap.
    impl CrowdRef {
        /// `new Crowd(grid, field, radius?)`: agents closer than twice `radius` (default 0.3 m) push apart.
        #[optional(radius = 0.3)]
        pub fn create(grid: NavGridRef, field: FlowFieldRef, radius: f64) -> CrowdRef {
            CROWDS.with(|all| {
                let mut all = all.borrow_mut();
                all.push(Some(CrowdEntry {
                    crowd: Crowd::new(radius as f32),
                    grid,
                    field,
                }));
                CrowdRef(all.len() as u32)
            })
        }

        /// Forgets the crowd (a handle is a number, nothing frees it by itself).
        pub fn destroy(self) {
            CROWDS.with(|all| {
                let mut all = all.borrow_mut();
                if let Some(slot) = (self.0 as usize)
                    .checked_sub(1)
                    .and_then(|i| all.get_mut(i))
                {
                    *slot = None;
                }
            });
        }

        /// Adds an agent at a world point with a top speed (metres per second); its number.
        pub fn add(self, p: V3, speed: f64) -> f64 {
            with_crowd(self, |e| e.crowd.add(vec(p), speed as f32) as f64).unwrap_or(-1.0)
        }

        /// Removes an agent; the last agent takes its number.
        pub fn remove(self, index: f64) {
            with_crowd(self, |e| e.crowd.remove(index as usize));
        }

        /// How many agents there are.
        #[property]
        pub fn count(self) -> f64 {
            with_crowd(self, |e| e.crowd.len() as f64).unwrap_or(0.0)
        }

        /// Follow a different field (a new goal, a rebuilt map).
        pub fn set_field(self, field: FlowFieldRef) {
            with_crowd(self, |e| e.field = field);
        }

        /// Agents this close to a goal (field distance, metres) stop; 0 means they only stop on it.
        pub fn set_goal_radius(self, radius: f64) {
            with_crowd(self, |e| e.crowd.goal_radius = radius as f32);
        }

        /// How hard agents push apart, as a fraction of their speed (default 1).
        pub fn set_separation(self, strength: f64) {
            with_crowd(self, |e| e.crowd.separation = strength as f32);
        }

        /// Moves every agent `dt` seconds.
        pub fn step(self, dt: f64) {
            CROWDS.with(|all| {
                let mut all = all.borrow_mut();
                let Some(Some(e)) = (self.0 as usize)
                    .checked_sub(1)
                    .and_then(|i| all.get_mut(i))
                else {
                    return;
                };
                FIELDS.with(|fields| {
                    let fields = fields.borrow();
                    let Some(Some(f)) = (e.field.0 as usize)
                        .checked_sub(1)
                        .and_then(|i| fields.get(i))
                    else {
                        return;
                    };
                    read_grid(e.grid, |g| e.crowd.step(&f.field, g, dt as f32));
                });
            });
        }

        /// Every agent's position, flat: x0, y0, z0, x1, y1, z1, ... (one call instead of one per agent).
        pub fn positions(self) -> Vec<f64> {
            with_crowd(self, |e| {
                e.crowd
                    .positions
                    .iter()
                    .flat_map(|p| [f64::from(p.x), f64::from(p.y), f64::from(p.z)])
                    .collect()
            })
            .unwrap_or_default()
        }

        /// One agent's position.
        pub fn position(self, index: f64) -> V3 {
            with_crowd(self, |e| e.crowd.positions.get(index as usize).copied())
                .flatten()
                .map_or(V3::ZERO, v3_of)
        }

        /// Puts an agent at a world point and stops it there (carrying it over a barrier it climbed).
        pub fn set_position(self, index: f64, p: V3) {
            with_crowd(self, |e| {
                let i = index as usize;
                if i < e.crowd.positions.len() {
                    e.crowd.positions[i] = vec(p);
                    e.crowd.velocities[i] = Vec3::ZERO;
                }
            });
        }

        /// One agent's velocity.
        pub fn velocity(self, index: f64) -> V3 {
            with_crowd(self, |e| e.crowd.velocities.get(index as usize).copied())
                .flatten()
                .map_or(V3::ZERO, v3_of)
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

    // AI-24
    #[test]
    fn goap_handle_plans_and_tells_no_plan_from_already_met() {
        reset();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let g = GoapRef::create();
        assert!(g.add_action("getAxe".into(), 1.0, &s(&["!axe"]), &s(&["axe"])));
        assert!(g.add_action("chop".into(), 2.0, &s(&["axe"]), &s(&["wood"])));
        assert!(!g.add_action("bad".into(), 1.0, &s(&["a", "!a"]), &[]));
        assert_eq!(g.plan(&[], &s(&["wood"])), ["getAxe", "chop"]);
        assert!(g.can_plan(&[], &s(&["wood"])));
        assert!(g.plan(&s(&["wood"]), &s(&["wood"])).is_empty());
        assert!(g.can_plan(&s(&["wood"]), &s(&["wood"])));
        assert!(!g.can_plan(&[], &s(&["moon"])));
        g.destroy();
        assert!(!g.can_plan(&[], &s(&["wood"])));
    }

    // AI-26
    #[test]
    fn crowd_handle_walks_agents_around_a_wall_to_the_goal() {
        reset();
        let grid = NavGridRef::create(v3(20.0, 1.0, 12.0), V3::ZERO, 1.0);
        grid.set_box_blocked(v3(9.0, 0.0, 0.0), v3(10.5, 0.9, 8.5), true);
        let goal = v3(17.5, 0.5, 2.5);
        let field = FlowFieldRef::create(grid, &[goal]);
        assert!(field.distance(v3(2.5, 0.5, 2.5)).is_finite());
        assert!(field.distance(v3(9.5, 0.5, 2.5)).is_infinite());
        assert!(field.direction(v3(9.5, 0.5, 2.5)) == V3::ZERO);
        let crowd = CrowdRef::create(grid, field, 0.3);
        crowd.set_goal_radius(2.5);
        for i in 0..12 {
            assert_eq!(
                crowd.add(
                    v3(2.0 + (i % 4) as f64 * 0.5, 0.5, 2.0 + (i / 4) as f64 * 0.5),
                    3.0
                ),
                i as f64
            );
        }
        assert_eq!(crowd.count(), 12.0);
        for _ in 0..900 {
            crowd.step(1.0 / 30.0);
        }
        let flat = crowd.positions();
        assert_eq!(flat.len(), 36);
        for i in 0..12 {
            let p = crowd.position(i as f64);
            assert!(!grid.is_blocked(grid.world_to_cell(p)));
            assert!(field.distance(p) < 5.0, "agent {i} stuck at {p:?}");
        }
        crowd.set_position(1.0, v3(5.5, 0.5, 5.5));
        assert_eq!(crowd.position(1.0), v3(5.5, 0.5, 5.5));
        assert_eq!(crowd.velocity(1.0), V3::ZERO);
        crowd.remove(0.0);
        assert_eq!(crowd.count(), 11.0);
        // Moving the goal and rebuilding changes the field.
        let before = field.distance(v3(2.5, 0.5, 2.5));
        field.rebuild(&[v3(2.5, 0.5, 10.5)]);
        assert!(field.distance(v3(2.5, 0.5, 2.5)) != before);
        crowd.destroy();
        field.destroy();
        assert_eq!(crowd.count(), 0.0);
        assert!(field.distance(v3(2.5, 0.5, 2.5)).is_infinite());
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
