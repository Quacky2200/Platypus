// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Goal-oriented action planning: facts are named booleans (at most 64), an action has
//! preconditions, effects and a cost, and the planner finds the cheapest action list that takes
//! the world from its state to one satisfying a goal (A* over world states).
//!
//! A *literal* is a fact name, or `!name` for "false". Facts a state doesn't list are false.

use crate::astar::{Graph, search};

/// The world as one bit per fact.
pub type State = u64;

#[derive(Clone, Debug)]
pub struct Action {
    pub name: String,
    pub cost: f32,
    /// Facts that must have the values in `pre_val`.
    pub pre_mask: u64,
    pub pre_val: u64,
    pub eff_mask: u64,
    pub eff_val: u64,
}

impl Action {
    pub fn applicable(&self, s: State) -> bool {
        s & self.pre_mask == self.pre_val
    }

    pub fn apply(&self, s: State) -> State {
        (s & !self.eff_mask) | self.eff_val
    }
}

/// A goal: the facts in `mask` must have the values in `val`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Goal {
    pub mask: u64,
    pub val: u64,
}

impl Goal {
    pub fn met(self, s: State) -> bool {
        s & self.mask == self.val
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Indices into [`Goap::actions`], in order.
    pub actions: Vec<usize>,
    pub cost: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Goap {
    facts: Vec<String>,
    pub actions: Vec<Action>,
    /// Gives up after this many expanded states.
    pub max_expansions: Option<usize>,
}

impl Goap {
    pub fn new() -> Self {
        Self::default()
    }

    /// The bit of a fact, registering it on first use; `None` once 64 facts exist.
    fn fact_bit(&mut self, name: &str) -> Option<u64> {
        let i = match self.facts.iter().position(|f| f == name) {
            Some(i) => i,
            None if self.facts.len() < 64 => {
                self.facts.push(name.to_string());
                self.facts.len() - 1
            }
            None => return None,
        };
        Some(1u64 << i)
    }

    /// `(mask, value)` bits of a list of literals.
    fn literals(&mut self, lits: &[&str]) -> Result<(u64, u64), String> {
        let (mut mask, mut val) = (0, 0);
        for lit in lits {
            let (name, want) = match lit.strip_prefix('!') {
                Some(n) => (n, false),
                None => (*lit, true),
            };
            let bit = self
                .fact_bit(name)
                .ok_or_else(|| format!("more than 64 facts (at `{name}`)"))?;
            if mask & bit != 0 && (val & bit != 0) != want {
                return Err(format!("`{name}` is both true and false"));
            }
            mask |= bit;
            if want {
                val |= bit;
            }
        }
        Ok((mask, val))
    }

    /// Adds an action; returns its index. Errors on a contradictory literal list or too many facts.
    pub fn add_action(
        &mut self,
        name: &str,
        cost: f32,
        preconditions: &[&str],
        effects: &[&str],
    ) -> Result<usize, String> {
        let (pre_mask, pre_val) = self.literals(preconditions)?;
        let (eff_mask, eff_val) = self.literals(effects)?;
        self.actions.push(Action {
            name: name.to_string(),
            cost: cost.max(0.0),
            pre_mask,
            pre_val,
            eff_mask,
            eff_val,
        });
        Ok(self.actions.len() - 1)
    }

    /// A state with exactly these facts true.
    pub fn state(&mut self, true_facts: &[&str]) -> Result<State, String> {
        self.literals(true_facts).map(|(_, v)| v)
    }

    pub fn goal(&mut self, literals: &[&str]) -> Result<Goal, String> {
        self.literals(literals)
            .map(|(mask, val)| Goal { mask, val })
    }

    /// The names of the facts true in `s`.
    pub fn facts_in(&self, s: State) -> Vec<&str> {
        self.facts
            .iter()
            .enumerate()
            .filter(|(i, _)| s >> i & 1 == 1)
            .map(|(_, f)| f.as_str())
            .collect()
    }

    /// The cheapest plan from `start` to a state meeting `goal`; empty when `start` already does,
    /// `None` when there is no plan.
    pub fn plan(&self, start: State, goal: Goal) -> Option<Plan> {
        // One action is needed while the goal is unmet, so the cheapest action is a safe estimate.
        let min_cost = self
            .actions
            .iter()
            .map(|a| a.cost)
            .fold(f32::INFINITY, f32::min);
        let found = search(
            &StateGraph(self),
            start,
            |s| goal.met(s),
            |s| if goal.met(s) { 0.0 } else { min_cost },
            self.max_expansions.unwrap_or(usize::MAX),
        )?;
        let mut actions = Vec::with_capacity(found.nodes.len().saturating_sub(1));
        for w in found.nodes.windows(2) {
            // The cheapest action that makes this step (several may).
            let (i, _) = self
                .actions
                .iter()
                .enumerate()
                .filter(|(_, a)| a.applicable(w[0]) && a.apply(w[0]) == w[1])
                .min_by(|a, b| a.1.cost.total_cmp(&b.1.cost))?;
            actions.push(i);
        }
        Some(Plan {
            actions,
            cost: found.cost,
        })
    }

    /// [`plan`](Self::plan) with names: the facts true now, the goal's literals; returns the action names.
    pub fn plan_names(
        &mut self,
        now: &[&str],
        goal: &[&str],
    ) -> Result<Option<Vec<String>>, String> {
        let start = self.state(now)?;
        let goal = self.goal(goal)?;
        Ok(self.plan(start, goal).map(|p| {
            p.actions
                .iter()
                .map(|&i| self.actions[i].name.clone())
                .collect()
        }))
    }
}

struct StateGraph<'a>(&'a Goap);

impl Graph for StateGraph<'_> {
    type Node = State;

    fn neighbors(&self, s: State, out: &mut Vec<(State, f32)>) {
        for a in &self.0.actions {
            if a.applicable(s) {
                let n = a.apply(s);
                // An action that changes nothing can only waste a step.
                if n != s {
                    out.push((n, a.cost));
                }
            }
        }
    }

    fn heuristic(&self, _from: State, _to: State) -> f32 {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soldier() -> Goap {
        let mut g = Goap::new();
        g.add_action("pickUpAxe", 1.0, &["!hasAxe"], &["hasAxe"])
            .unwrap();
        g.add_action("chopTree", 2.0, &["hasAxe"], &["hasWood"])
            .unwrap();
        g.add_action("buyWood", 10.0, &["hasCoins"], &["hasWood"])
            .unwrap();
        g.add_action("buildFire", 1.0, &["hasWood"], &["fire", "!hasWood"])
            .unwrap();
        g
    }

    // AI-30
    #[test]
    fn the_cheapest_plan_is_found() {
        let mut g = soldier();
        let plan = g.plan_names(&[], &["fire"]).unwrap().unwrap();
        assert_eq!(plan, ["pickUpAxe", "chopTree", "buildFire"]);
        // With coins and an axe already, chopping (2) beats buying (10).
        let plan = g
            .plan_names(&["hasAxe", "hasCoins"], &["fire"])
            .unwrap()
            .unwrap();
        assert_eq!(plan, ["chopTree", "buildFire"]);
    }

    // AI-31
    #[test]
    fn costs_pick_between_routes() {
        let mut g = soldier();
        g.actions[0].cost = 50.0; // the axe is far away: buying wood is cheaper now
        let plan = g.plan_names(&["hasCoins"], &["fire"]).unwrap().unwrap();
        assert_eq!(plan, ["buyWood", "buildFire"]);
    }

    // AI-32
    #[test]
    fn met_goals_need_no_actions_and_impossible_ones_no_plan() {
        let mut g = soldier();
        assert_eq!(g.plan_names(&["fire"], &["fire"]).unwrap(), Some(vec![]));
        assert_eq!(g.plan_names(&[], &["dragonSlain"]).unwrap(), None);
    }

    // AI-33
    #[test]
    fn negative_goals_and_effects_work() {
        let mut g = soldier();
        let plan = g
            .plan_names(&["hasWood"], &["fire", "!hasWood"])
            .unwrap()
            .unwrap();
        assert_eq!(plan, ["buildFire"]);
    }

    // AI-34
    #[test]
    fn bad_literals_and_too_many_facts_are_errors() {
        let mut g = Goap::new();
        assert!(g.add_action("x", 1.0, &["a", "!a"], &[]).is_err());
        // The failed action above still registered its fact `a`.
        for i in 0..63 {
            g.state(&[&format!("f{i}")]).unwrap();
        }
        assert!(g.state(&["one-too-many"]).is_err());
    }

    // AI-35
    #[test]
    fn the_expansion_limit_gives_up() {
        let mut g = soldier();
        g.max_expansions = Some(1);
        assert_eq!(g.plan_names(&[], &["fire"]).unwrap(), None);
    }
}
