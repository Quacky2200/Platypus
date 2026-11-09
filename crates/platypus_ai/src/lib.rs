// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Bot AI in plain Rust: pathfinding and steering for 3D worlds.
//!
//! - [`astar`]: A* over any [`astar::Graph`].
//! - [`grid`]: [`grid::NavGrid`], a 3D cell grid with 6/18/26 connectivity, per-cell costs,
//!   line of sight and path smoothing.
//! - [`flow`]: flow fields over a grid and a [`flow::Crowd`] that moves a swarm along one.
//! - [`goap`]: goal-oriented action planning (named boolean facts, A* over world states).
//! - [`steering`]: seek, flee, arrive, pursue, evade, separation and path following.
//!
//! Later: GOAP, flow fields for crowds (rats), small neural nets.

pub mod astar;
pub mod flow;
pub mod goap;
pub mod grid;
pub mod steering;

pub use astar::{Graph, Path, find_path};
pub use flow::{Crowd, FlowField};
pub use goap::Goap;
pub use grid::{Cell, Connectivity, NavGrid};

#[cfg(test)]
mod tests;
