// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use crate::steering::{Agent, PathFollower};
use crate::*;
use glam::{IVec3, Vec3};

fn grid(n: i32) -> NavGrid {
    NavGrid::new(IVec3::splat(n), Vec3::ZERO, 1.0)
}

// AI-01
#[test]
fn open_grid_path_is_the_straight_diagonal() {
    let g = grid(8);
    let p = g
        .find_cell_path(IVec3::ZERO, IVec3::splat(7))
        .expect("path");
    assert_eq!(p.len(), 8);
    assert_eq!(p[0], IVec3::ZERO);
    assert_eq!(*p.last().unwrap(), IVec3::splat(7));
}

// AI-02
#[test]
fn six_connectivity_costs_manhattan_distance() {
    let mut g = grid(8);
    g.connectivity = Connectivity::Six;
    let path = find_path(&g, IVec3::ZERO, IVec3::new(3, 2, 1)).expect("path");
    assert!((path.cost - 6.0).abs() < 1e-4);
}

// AI-03
#[test]
fn walls_are_walked_around_and_sealed_goals_have_no_path() {
    let mut g = grid(8);
    // A wall across x = 3 with a hole at the far corner.
    for y in 0..8 {
        for z in 0..8 {
            g.set_blocked(IVec3::new(3, y, z), true);
        }
    }
    assert!(g.find_cell_path(IVec3::ZERO, IVec3::new(6, 0, 0)).is_none());
    g.set_blocked(IVec3::new(3, 7, 7), false);
    let p = g.find_cell_path(IVec3::ZERO, IVec3::new(6, 0, 0)).unwrap();
    assert!(p.contains(&IVec3::new(3, 7, 7)));
    assert!(p.iter().all(|&c| !g.is_blocked(c)));
}

// AI-04
#[test]
fn diagonals_do_not_cut_blocked_corners() {
    // One cell thick, so there is no way around over the z layer.
    let mut g = NavGrid::new(IVec3::new(4, 4, 1), Vec3::ZERO, 1.0);
    g.set_blocked(IVec3::new(1, 0, 0), true);
    g.set_blocked(IVec3::new(0, 1, 0), true);
    assert!(g.find_cell_path(IVec3::ZERO, IVec3::new(1, 1, 0)).is_none());
    g.corner_cutting = true;
    assert!(g.find_cell_path(IVec3::ZERO, IVec3::new(1, 1, 0)).is_some());
}

// AI-05
#[test]
fn weights_steer_paths_around_expensive_cells() {
    let mut g = grid(5);
    g.connectivity = Connectivity::Six;
    for x in 1..4 {
        g.set_weight(IVec3::new(x, 0, 0), 10.0);
    }
    let p = g.find_cell_path(IVec3::ZERO, IVec3::new(4, 0, 0)).unwrap();
    assert!(p.iter().skip(1).take(3).all(|c| c.y != 0 || c.x == 0));
}

// AI-06
#[test]
fn line_of_sight_sees_open_space_and_stops_at_blocks() {
    let mut g = grid(8);
    let a = Vec3::new(0.5, 0.5, 0.5);
    let b = Vec3::new(7.5, 6.5, 3.5);
    assert!(g.line_of_sight(a, b));
    g.set_blocked(IVec3::new(4, 3, 2), true);
    // The straight line passes through some cell near the middle; block a ring of them.
    g.set_box_blocked(Vec3::new(3.0, 2.0, 1.0), Vec3::new(5.0, 4.0, 3.0), true);
    assert!(!g.line_of_sight(a, b));
}

// AI-07
#[test]
fn smoothing_collapses_an_open_path_to_two_points() {
    let g = grid(8);
    let p = g
        .find_path(Vec3::new(0.5, 0.5, 0.5), Vec3::new(7.5, 3.5, 5.5), true)
        .unwrap();
    assert_eq!(p.len(), 2);
    assert_eq!(p[0], Vec3::new(0.5, 0.5, 0.5));
    assert_eq!(p[1], Vec3::new(7.5, 3.5, 5.5));
}

// AI-08
#[test]
fn expansion_limit_gives_up() {
    let mut g = grid(16);
    g.max_expansions = 3;
    assert!(g.find_cell_path(IVec3::ZERO, IVec3::splat(15)).is_none());
}

// AI-09
#[test]
fn nearest_free_leaves_a_blocked_cell() {
    let mut g = grid(5);
    g.set_blocked(IVec3::splat(2), true);
    let c = g.nearest_free(IVec3::splat(2), 2).unwrap();
    assert!(!g.is_blocked(c));
    assert_eq!((c - IVec3::splat(2)).abs().element_sum(), 1);
}

// AI-10
#[test]
fn arrive_comes_to_rest_on_the_target() {
    let mut a = Agent::new(Vec3::ZERO, 5.0, 20.0);
    let target = Vec3::new(10.0, 0.0, 0.0);
    for _ in 0..600 {
        let acc = a.arrive(target, 3.0);
        a.integrate(acc, 1.0 / 60.0);
    }
    assert!((a.position - target).length() < 0.2);
    assert!(a.velocity.length() < 0.3);
}

// AI-11
#[test]
fn flee_and_separation_move_away() {
    let a = Agent::new(Vec3::ZERO, 5.0, 20.0);
    assert!(a.flee(Vec3::X).x < 0.0);
    assert!(a.separation(&[Vec3::new(0.5, 0.0, 0.0)], 2.0).x < 0.0);
    assert_eq!(a.separation(&[Vec3::new(5.0, 0.0, 0.0)], 2.0), Vec3::ZERO);
    // A moving agent with nobody close gets no braking force from it.
    let mut moving = Agent::new(Vec3::ZERO, 5.0, 20.0);
    moving.velocity = Vec3::new(3.0, 0.0, 0.0);
    assert_eq!(moving.separation(&[], 2.0), Vec3::ZERO);
    assert!(moving.separation(&[Vec3::new(0.5, 0.0, 0.0)], 2.0).length() <= 20.0);
}

// AI-12
#[test]
fn pursue_leads_a_moving_target() {
    let a = Agent::new(Vec3::ZERO, 5.0, 20.0);
    let acc = a.pursue(Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 5.0));
    assert!(acc.z > 0.0 && acc.x > 0.0);
}

// AI-13
#[test]
fn an_agent_follows_a_path_around_a_wall_to_the_end() {
    let mut g = grid(10);
    g.set_box_blocked(Vec3::new(5.0, 0.0, 0.0), Vec3::new(5.5, 7.0, 9.5), true);
    let from = Vec3::new(1.5, 1.5, 1.5);
    let to = Vec3::new(8.5, 1.5, 1.5);
    let pts = g.find_path(from, to, true).unwrap();
    let mut follower = PathFollower::new(pts, 0.3);
    let mut a = Agent::new(from, 4.0, 30.0);
    for _ in 0..2000 {
        match follower.steer(&a, 1.0) {
            Some(acc) => a.integrate(acc, 1.0 / 60.0),
            None => break,
        }
        assert!(!g.is_blocked(g.world_to_cell(a.position)));
    }
    assert!(follower.finished());
    assert!((a.position - to).length() < 0.5);
}
