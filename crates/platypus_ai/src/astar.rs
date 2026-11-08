// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A* over any graph.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;

/// A graph A* can search. Edge costs must be non-negative and `heuristic` must never
/// overestimate the true cost, or the path may not be the cheapest.
pub trait Graph {
    type Node: Copy + Eq + Hash;
    /// Pushes every `(neighbour, edge cost)` of `node` onto `out`.
    fn neighbors(&self, node: Self::Node, out: &mut Vec<(Self::Node, f32)>);
    /// Estimated cost from `from` to `to`.
    fn heuristic(&self, from: Self::Node, to: Self::Node) -> f32;
}

/// A found path, start and goal included.
#[derive(Clone, Debug, PartialEq)]
pub struct Path<N> {
    pub nodes: Vec<N>,
    pub cost: f32,
}

struct Open<N> {
    f: f32,
    g: f32,
    node: N,
}

impl<N> PartialEq for Open<N> {
    fn eq(&self, o: &Self) -> bool {
        self.f.total_cmp(&o.f).is_eq()
    }
}
impl<N> Eq for Open<N> {}
impl<N> PartialOrd for Open<N> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl<N> Ord for Open<N> {
    // Reversed: BinaryHeap pops the largest, we want the smallest f; ties prefer deeper nodes.
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then(self.g.total_cmp(&o.g))
    }
}

/// The cheapest path from `start` to `goal`, or `None` when there is none or the search
/// expanded more than `max_expansions` nodes (`usize::MAX` for no limit).
pub fn find_path_limited<G: Graph>(
    graph: &G,
    start: G::Node,
    goal: G::Node,
    max_expansions: usize,
) -> Option<Path<G::Node>> {
    let mut open = BinaryHeap::new();
    let mut best: HashMap<G::Node, f32> = HashMap::new();
    let mut came: HashMap<G::Node, G::Node> = HashMap::new();
    let mut edges = Vec::new();
    best.insert(start, 0.0);
    open.push(Open {
        f: graph.heuristic(start, goal),
        g: 0.0,
        node: start,
    });
    let mut expanded = 0usize;
    while let Some(Open { g, node, .. }) = open.pop() {
        if node == goal {
            let mut nodes = vec![node];
            let mut at = node;
            while let Some(&p) = came.get(&at) {
                nodes.push(p);
                at = p;
            }
            nodes.reverse();
            return Some(Path { nodes, cost: g });
        }
        // A stale queue entry: a cheaper way here was found after it was pushed.
        if best.get(&node).is_some_and(|&b| g > b) {
            continue;
        }
        expanded += 1;
        if expanded > max_expansions {
            return None;
        }
        edges.clear();
        graph.neighbors(node, &mut edges);
        for &(next, cost) in &edges {
            let ng = g + cost;
            if best.get(&next).is_none_or(|&b| ng < b) {
                best.insert(next, ng);
                came.insert(next, node);
                open.push(Open {
                    f: ng + graph.heuristic(next, goal),
                    g: ng,
                    node: next,
                });
            }
        }
    }
    None
}

/// [`find_path_limited`] with no expansion limit.
pub fn find_path<G: Graph>(graph: &G, start: G::Node, goal: G::Node) -> Option<Path<G::Node>> {
    find_path_limited(graph, start, goal, usize::MAX)
}
