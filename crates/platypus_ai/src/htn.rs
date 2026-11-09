// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Hierarchical task networks. Where GOAP searches for the cheapest way to a goal, an HTN follows
//! recipes its author wrote: a *compound* task (`LightFire`) has *methods*, tried in order, each with
//! conditions and a list of subtasks (compound or primitive) to do instead; *primitive* tasks are
//! the actual steps, with preconditions, effects and a cost like GOAP actions. The planner decomposes
//! the root task depth first, in order, simulating the world through each primitive, and backs up to
//! the next method when a branch fails.
//!
//! So the result is the first plan the author's priorities allow, not the cheapest: put the preferred
//! method first, and a catch-all with no conditions last. Facts are the same named booleans as GOAP
//! ([`crate::facts`]); a method's conditions are checked against the world as the earlier steps of the
//! plan will have left it.

use crate::facts::{Facts, State};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskRef {
    Primitive(usize),
    Compound(usize),
}

/// A step: what the plan is made of.
#[derive(Clone, Debug)]
pub struct Primitive {
    pub name: String,
    pub cost: f32,
    pre_mask: u64,
    pre_val: u64,
    eff_mask: u64,
    eff_val: u64,
}

impl Primitive {
    pub fn applicable(&self, s: State) -> bool {
        s & self.pre_mask == self.pre_val
    }

    pub fn apply(&self, s: State) -> State {
        (s & !self.eff_mask) | self.eff_val
    }
}

/// One way to do a compound task: used when its conditions hold, then replaced by its subtasks.
#[derive(Clone, Debug)]
pub struct Method {
    pub name: String,
    pre_mask: u64,
    pre_val: u64,
    /// Task names, in order (may be defined after the method; one that never is makes the method fail).
    pub subtasks: Vec<String>,
}

/// A task made of other tasks: its methods are tried in the order they were added.
#[derive(Clone, Debug)]
pub struct Compound {
    pub name: String,
    pub methods: Vec<Method>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Indices into [`Htn::primitives`], in order.
    pub actions: Vec<usize>,
    pub cost: f32,
    /// The methods the decomposition used, in order, as `"Compound: method"`.
    pub methods: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Htn {
    facts: Facts,
    pub primitives: Vec<Primitive>,
    pub compounds: Vec<Compound>,
    index: HashMap<String, TaskRef>,
    /// Gives up after this many tasks have been processed (a recursive method that never ends).
    pub max_steps: usize,
    /// A branch with more than this many tasks waiting is abandoned (left recursion).
    pub max_depth: usize,
}

impl Default for Htn {
    fn default() -> Self {
        Htn {
            facts: Facts::default(),
            primitives: Vec::new(),
            compounds: Vec::new(),
            index: HashMap::new(),
            max_steps: 20_000,
            max_depth: 64,
        }
    }
}

struct Choice {
    todo: Vec<TaskRef>,
    state: State,
    out: usize,
    methods: usize,
    compound: usize,
    next: usize,
}

struct Search {
    /// Tasks still to do; the last is next.
    todo: Vec<TaskRef>,
    state: State,
    out: Vec<usize>,
    methods: Vec<String>,
    choices: Vec<Choice>,
}

impl Htn {
    pub fn new() -> Self {
        Self::default()
    }

    fn check_free(&self, name: &str) -> Result<(), String> {
        if self.index.contains_key(name) {
            Err(format!("a task called `{name}` already exists"))
        } else {
            Ok(())
        }
    }

    /// Adds a step. Errors on a name already used, a contradictory literal list or too many facts.
    pub fn add_primitive(
        &mut self,
        name: &str,
        cost: f32,
        preconditions: &[&str],
        effects: &[&str],
    ) -> Result<(), String> {
        self.check_free(name)?;
        let (pre_mask, pre_val) = self.facts.literals(preconditions)?;
        let (eff_mask, eff_val) = self.facts.literals(effects)?;
        self.primitives.push(Primitive {
            name: name.to_string(),
            cost: cost.max(0.0),
            pre_mask,
            pre_val,
            eff_mask,
            eff_val,
        });
        self.index.insert(
            name.to_string(),
            TaskRef::Primitive(self.primitives.len() - 1),
        );
        Ok(())
    }

    /// Adds a compound task with no methods yet.
    pub fn add_compound(&mut self, name: &str) -> Result<(), String> {
        self.check_free(name)?;
        self.compounds.push(Compound {
            name: name.to_string(),
            methods: Vec::new(),
        });
        self.index.insert(
            name.to_string(),
            TaskRef::Compound(self.compounds.len() - 1),
        );
        Ok(())
    }

    /// Adds a method to a compound task, after its earlier ones (so with lower priority).
    pub fn add_method(
        &mut self,
        compound: &str,
        name: &str,
        preconditions: &[&str],
        subtasks: &[&str],
    ) -> Result<(), String> {
        let Some(TaskRef::Compound(c)) = self.index.get(compound).copied() else {
            return Err(format!("`{compound}` is not a compound task"));
        };
        let (pre_mask, pre_val) = self.facts.literals(preconditions)?;
        self.compounds[c].methods.push(Method {
            name: name.to_string(),
            pre_mask,
            pre_val,
            subtasks: subtasks.iter().map(|s| s.to_string()).collect(),
        });
        Ok(())
    }

    /// A state with exactly these facts true.
    pub fn state(&mut self, true_facts: &[&str]) -> Result<State, String> {
        self.facts.literals(true_facts).map(|(_, v)| v)
    }

    /// The names of the facts true in `s`.
    pub fn facts_in(&self, s: State) -> Vec<&str> {
        self.facts.names_in(s)
    }

    /// Subtask names no task is defined for (such methods can never be used).
    pub fn unknown_subtasks(&self) -> Vec<String> {
        let mut out = Vec::new();
        for c in &self.compounds {
            for m in &c.methods {
                for s in &m.subtasks {
                    if !self.index.contains_key(s) && !out.contains(s) {
                        out.push(s.clone());
                    }
                }
            }
        }
        out
    }

    /// Replaces the compound task `c` (already taken off the list) with the subtasks of its first
    /// method at or after `from` whose conditions hold now. Leaves a choice point when later methods
    /// remain. False when no method fits.
    fn expand(&self, s: &mut Search, c: usize, from: usize) -> bool {
        let comp = &self.compounds[c];
        for (mi, m) in comp.methods.iter().enumerate().skip(from) {
            if s.state & m.pre_mask != m.pre_val {
                continue;
            }
            let Some(subs) = m
                .subtasks
                .iter()
                .map(|n| self.index.get(n).copied())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if mi + 1 < comp.methods.len() {
                s.choices.push(Choice {
                    todo: s.todo.clone(),
                    state: s.state,
                    out: s.out.len(),
                    methods: s.methods.len(),
                    compound: c,
                    next: mi + 1,
                });
            }
            s.methods.push(format!("{}: {}", comp.name, m.name));
            s.todo.extend(subs.into_iter().rev());
            return true;
        }
        false
    }

    /// Decomposes `root` from the world `start`: the first plan the methods' order allows (empty when
    /// the root needs no steps), `None` when every way fails or the step budget runs out. Errors on a
    /// root that is not a task.
    pub fn plan(&self, start: State, root: &str) -> Result<Option<Plan>, String> {
        let root = *self
            .index
            .get(root)
            .ok_or_else(|| format!("no task called `{root}`"))?;
        let mut s = Search {
            todo: vec![root],
            state: start,
            out: Vec::new(),
            methods: Vec::new(),
            choices: Vec::new(),
        };
        let mut steps = 0usize;
        loop {
            steps += 1;
            if steps > self.max_steps {
                return Ok(None);
            }
            let mut failed = s.todo.len() > self.max_depth;
            if !failed {
                match s.todo.pop() {
                    None => {
                        let cost = s.out.iter().map(|&i| self.primitives[i].cost).sum();
                        return Ok(Some(Plan {
                            actions: s.out,
                            cost,
                            methods: s.methods,
                        }));
                    }
                    Some(TaskRef::Primitive(i)) => {
                        let p = &self.primitives[i];
                        if p.applicable(s.state) {
                            s.out.push(i);
                            s.state = p.apply(s.state);
                        } else {
                            failed = true;
                        }
                    }
                    Some(TaskRef::Compound(c)) => {
                        failed = !self.expand(&mut s, c, 0);
                    }
                }
            }
            // A dead end: back up to the latest choice with another method to try.
            while failed {
                let Some(ch) = s.choices.pop() else {
                    return Ok(None);
                };
                s.todo = ch.todo;
                s.state = ch.state;
                s.out.truncate(ch.out);
                s.methods.truncate(ch.methods);
                failed = !self.expand(&mut s, ch.compound, ch.next);
            }
        }
    }

    /// [`plan`](Self::plan) with names: the facts true now and the root task; returns the step names.
    pub fn plan_names(&mut self, now: &[&str], root: &str) -> Result<Option<Plan>, String> {
        let start = self.state(now)?;
        self.plan(start, root)
    }

    /// The names of a plan's steps.
    pub fn step_names(&self, plan: &Plan) -> Vec<String> {
        plan.actions
            .iter()
            .map(|&i| self.primitives[i].name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small village: get wood by the best route you can, then light a fire.
    fn village() -> Htn {
        let mut h = Htn::new();
        for p in ["shed", "forest", "market", "camp"] {
            h.add_primitive(&format!("go:{p}"), 2.0, &[], &[&format!("at:{p}")])
                .unwrap();
            h.add_compound(&format!("At:{p}")).unwrap();
            h.add_method(
                &format!("At:{p}"),
                "already there",
                &[&format!("at:{p}")],
                &[],
            )
            .unwrap();
            h.add_method(&format!("At:{p}"), "walk", &[], &[&format!("go:{p}")])
                .unwrap();
        }
        h.add_primitive("takeAxe", 1.0, &["at:shed"], &["hasAxe"])
            .unwrap();
        h.add_primitive("chopWood", 3.0, &["at:forest", "hasAxe"], &["hasWood"])
            .unwrap();
        h.add_primitive(
            "buyWood",
            5.0,
            &["at:market", "hasCoins"],
            &["hasWood", "!hasCoins"],
        )
        .unwrap();
        h.add_primitive(
            "buildFire",
            2.0,
            &["at:camp", "hasWood"],
            &["fire", "!hasWood"],
        )
        .unwrap();

        h.add_compound("GetAxe").unwrap();
        h.add_method("GetAxe", "have it", &["hasAxe"], &[]).unwrap();
        h.add_method("GetAxe", "fetch it", &[], &["At:shed", "takeAxe"])
            .unwrap();

        h.add_compound("GetWood").unwrap();
        h.add_method("GetWood", "have some", &["hasWood"], &[])
            .unwrap();
        h.add_method(
            "GetWood",
            "buy it",
            &["hasCoins"],
            &["At:market", "buyWood"],
        )
        .unwrap();
        h.add_method(
            "GetWood",
            "chop it",
            &[],
            &["GetAxe", "At:forest", "chopWood"],
        )
        .unwrap();

        h.add_compound("LightFire").unwrap();
        h.add_method(
            "LightFire",
            "the usual",
            &[],
            &["GetWood", "At:camp", "buildFire"],
        )
        .unwrap();
        h
    }

    fn steps(h: &mut Htn, now: &[&str], root: &str) -> Option<Vec<String>> {
        let p = h.plan_names(now, root).unwrap()?;
        Some(h.step_names(&p))
    }

    // AI-50
    #[test]
    fn methods_are_tried_in_order_and_the_first_that_works_wins() {
        let mut h = village();
        // Empty-handed: chop it (the last method), via the shed for the axe.
        assert_eq!(
            steps(&mut h, &[], "LightFire").unwrap(),
            [
                "go:shed",
                "takeAxe",
                "go:forest",
                "chopWood",
                "go:camp",
                "buildFire"
            ]
        );
        // With a coin, "buy it" comes first and wins, though chopping would be cheaper here:
        // it is the author's priority, not the cheapest.
        let p = h
            .plan_names(&["hasCoins", "hasAxe", "at:forest"], "LightFire")
            .unwrap()
            .unwrap();
        assert_eq!(
            h.step_names(&p),
            ["go:market", "buyWood", "go:camp", "buildFire"]
        );
        assert!(p.cost > 8.0);
    }

    // AI-51
    #[test]
    fn conditions_see_the_world_the_earlier_steps_leave() {
        let mut h = village();
        // Standing at the camp already: no walking there, whatever came before.
        let got = steps(&mut h, &["at:camp", "hasWood"], "LightFire").unwrap();
        assert_eq!(got, ["buildFire"]);
        // After fetching the axe at the shed, GetAxe's "have it" is not needed again: the chop
        // route reads hasAxe set by takeAxe.
        let got = steps(&mut h, &["at:shed"], "LightFire").unwrap();
        assert_eq!(got[0], "takeAxe");
    }

    // AI-52
    #[test]
    fn a_failed_branch_backs_up_to_the_next_method() {
        let mut h = Htn::new();
        h.add_primitive("a", 1.0, &["never"], &["x"]).unwrap();
        h.add_primitive("b", 1.0, &[], &["x"]).unwrap();
        h.add_primitive("c", 1.0, &["x"], &["done"]).unwrap();
        h.add_compound("Root").unwrap();
        // The first method's conditions hold but its step cannot run; the second works.
        h.add_method("Root", "first", &[], &["a", "c"]).unwrap();
        h.add_method("Root", "second", &[], &["b", "c"]).unwrap();
        let p = h.plan_names(&[], "Root").unwrap().unwrap();
        assert_eq!(h.step_names(&p), ["b", "c"]);
        assert_eq!(p.methods, ["Root: second"]);
        // Only a later step fails, deeper down: still backs up through the outer choice.
        let mut h = Htn::new();
        h.add_primitive("ok", 1.0, &[], &["y"]).unwrap();
        h.add_primitive("needsZ", 1.0, &["z"], &["w"]).unwrap();
        h.add_primitive("fine", 1.0, &[], &["w"]).unwrap();
        h.add_compound("Root").unwrap();
        h.add_method("Root", "m1", &[], &["ok", "needsZ"]).unwrap();
        h.add_method("Root", "m2", &[], &["ok", "fine"]).unwrap();
        let p = h.plan_names(&[], "Root").unwrap().unwrap();
        assert_eq!(h.step_names(&p), ["ok", "fine"]);
    }

    // AI-53
    #[test]
    fn recursion_works_and_runaway_recursion_gives_up() {
        let mut h = Htn::new();
        h.add_primitive("step", 1.0, &["!done"], &["hop"]).unwrap();
        h.add_primitive("finish", 1.0, &["hop"], &["done"]).unwrap();
        h.add_compound("Loop").unwrap();
        h.add_method("Loop", "stop", &["done"], &[]).unwrap();
        h.add_method("Loop", "again", &["!hop"], &["step", "Loop"])
            .unwrap();
        h.add_method("Loop", "finish up", &["hop"], &["finish", "Loop"])
            .unwrap();
        let p = h.plan_names(&[], "Loop").unwrap().unwrap();
        assert_eq!(h.step_names(&p), ["step", "finish"]);

        // A method that only calls itself never ends: the budget stops it.
        let mut h = Htn::new();
        h.add_compound("Forever").unwrap();
        h.add_method("Forever", "again", &[], &["Forever"]).unwrap();
        h.max_steps = 500;
        assert!(h.plan_names(&[], "Forever").unwrap().is_none());
        // Left recursion that keeps growing the list is cut by the depth limit.
        let mut h = Htn::new();
        h.add_primitive("p", 1.0, &[], &[]).unwrap();
        h.add_compound("Grow").unwrap();
        h.add_method("Grow", "more", &[], &["Grow", "p"]).unwrap();
        assert!(h.plan_names(&[], "Grow").unwrap().is_none());
    }

    // AI-54
    #[test]
    fn empty_plans_unknown_names_and_bad_input() {
        let mut h = village();
        let p = h
            .plan_names(&["hasWood", "hasAxe"], "GetWood")
            .unwrap()
            .unwrap();
        assert!(p.actions.is_empty());
        assert_eq!(p.cost, 0.0);
        assert!(h.plan_names(&[], "Nope").is_err());
        assert!(
            h.add_primitive("takeAxe", 1.0, &[], &[]).is_err(),
            "names are unique"
        );
        assert!(h.add_compound("GetWood").is_err());
        assert!(
            h.add_method("takeAxe", "m", &[], &[]).is_err(),
            "only compounds have methods"
        );
        assert!(h.add_primitive("bad", 1.0, &["a", "!a"], &[]).is_err());
        // A method naming a task that never exists is skipped, and reported.
        h.add_compound("Odd").unwrap();
        h.add_method("Odd", "ghost", &[], &["Missing"]).unwrap();
        h.add_method("Odd", "real", &[], &["takeAxe"]).unwrap();
        assert_eq!(h.unknown_subtasks(), ["Missing"]);
        let p = h.plan_names(&["at:shed"], "Odd").unwrap().unwrap();
        assert_eq!(h.step_names(&p), ["takeAxe"]);
    }

    // AI-55
    #[test]
    fn the_plan_records_the_methods_it_used() {
        let mut h = village();
        let p = h.plan_names(&["hasCoins"], "LightFire").unwrap().unwrap();
        assert_eq!(
            p.methods,
            [
                "LightFire: the usual",
                "GetWood: buy it",
                "At:market: walk",
                "At:camp: walk"
            ]
        );
    }
}
