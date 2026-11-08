// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Bot AI in plain Rust: pathfinding and steering for 3D worlds.
//!
//! - [`astar`]: A* over any [`astar::Graph`].
//! - [`grid`]: [`grid::NavGrid`], a 3D cell grid with 6/18/26 connectivity, per-cell costs,
//!   line of sight and path smoothing.
//! - [`steering`]: seek, flee, arrive, pursue, evade, separation and path following.
//!
//! Later: GOAP, flow fields for crowds (rats), small neural nets.

pub mod astar;
pub mod grid;
pub mod steering;

pub use astar::{Graph, Path, find_path};
pub use grid::{Cell, Connectivity, NavGrid};

#[cfg(test)]
mod tests;
