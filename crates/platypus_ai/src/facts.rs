// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The named boolean facts the symbolic planners ([`crate::goap`], [`crate::htn`]) share: at most
//! 64, one bit each in a [`State`].
//!
//! A *literal* is a fact name, or `!name` for "false". Facts a state doesn't list are false.

/// The world as one bit per fact.
pub type State = u64;

/// The registry of fact names.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    names: Vec<String>,
}

impl Facts {
    /// The bit of a fact, registering it on first use; `None` once 64 facts exist.
    pub fn bit(&mut self, name: &str) -> Option<u64> {
        let i = match self.names.iter().position(|f| f == name) {
            Some(i) => i,
            None if self.names.len() < 64 => {
                self.names.push(name.to_string());
                self.names.len() - 1
            }
            None => return None,
        };
        Some(1u64 << i)
    }

    /// `(mask, value)` bits of a list of literals. Errors when a fact is both true and false in the
    /// list, or there are more than 64 facts.
    pub fn literals(&mut self, lits: &[&str]) -> Result<(u64, u64), String> {
        let (mut mask, mut val) = (0, 0);
        for lit in lits {
            let (name, want) = match lit.strip_prefix('!') {
                Some(n) => (n, false),
                None => (*lit, true),
            };
            let bit = self
                .bit(name)
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

    /// The names of the facts true in `s`.
    pub fn names_in(&self, s: State) -> Vec<&str> {
        self.names
            .iter()
            .enumerate()
            .filter(|(i, _)| s >> i & 1 == 1)
            .map(|(_, f)| f.as_str())
            .collect()
    }
}
