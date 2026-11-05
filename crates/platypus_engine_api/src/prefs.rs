// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `PlayerPrefs`: numbers and strings by key (Unity's). The host loads the saved values at start
//! ([`load`]) and applies what scripts changed ([`take_changes`]) to the file; keys starting with
//! `platypus.` are the engine's own (window mode, quality level).

use std::cell::RefCell;
use std::collections::BTreeMap;
use ts_api::ts_api;

#[derive(Clone, Debug, PartialEq)]
pub enum Pref {
    Num(f64),
    Str(String),
}

/// What a script did, in order, for the host to write down.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Set(String, Pref),
    Delete(String),
    DeleteAll,
    Save,
}

#[derive(Default)]
struct State {
    values: BTreeMap<String, Pref>,
    changes: Vec<Change>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Replaces the values with the saved ones (at start; records no change).
pub fn load(entries: impl IntoIterator<Item = (String, Pref)>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.values = entries.into_iter().collect();
        s.changes.clear();
    });
}

/// The changes scripts made since the last call.
pub fn take_changes() -> Vec<Change> {
    STATE.with(|s| std::mem::take(&mut s.borrow_mut().changes))
}

pub fn get(key: &str) -> Option<Pref> {
    STATE.with(|s| s.borrow().values.get(key).cloned())
}

pub fn set(key: &str, value: Pref) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.values.insert(key.to_string(), value.clone());
        s.changes.push(Change::Set(key.to_string(), value));
    });
}

/// A JSON text kept under an engine key (`platypus.quality`).
pub fn json(key: &str) -> Option<String> {
    match get(key) {
        Some(Pref::Str(s)) => Some(s),
        _ => None,
    }
}

pub fn set_json(key: &str, text: &str) {
    set(key, Pref::Str(text.to_string()));
}

/// Forgets everything (a test).
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}

fn number(key: &str) -> Option<f64> {
    match get(key) {
        Some(Pref::Num(n)) => Some(n),
        _ => None,
    }
}

#[ts_api(namespace = "PlayerPrefs")]
pub mod prefs_api {
    use super::{Change, Pref, STATE, get, number, set};

    /// A whole number (`fallback`, or 0, when there is none).
    #[optional(fallback)]
    pub fn get_int(key: &str, fallback: f64) -> f64 {
        match number(key) {
            Some(n) => n.trunc(),
            None if fallback.is_nan() => 0.0,
            None => fallback,
        }
    }

    #[optional(fallback)]
    pub fn get_float(key: &str, fallback: f64) -> f64 {
        match number(key) {
            Some(n) => n,
            None if fallback.is_nan() => 0.0,
            None => fallback,
        }
    }

    #[optional(fallback)]
    pub fn get_string(key: &str, fallback: &str) -> String {
        match get(key) {
            Some(Pref::Str(s)) => s,
            _ => fallback.to_string(),
        }
    }

    pub fn set_int(key: &str, value: f64) {
        set(key, Pref::Num(value.trunc()));
    }

    pub fn set_float(key: &str, value: f64) {
        set(key, Pref::Num(value));
    }

    pub fn set_string(key: &str, value: &str) {
        set(key, Pref::Str(value.to_string()));
    }

    pub fn has_key(key: &str) -> bool {
        get(key).is_some()
    }

    pub fn delete_key(key: &str) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.values.remove(key);
            s.changes.push(Change::Delete(key.to_string()));
        });
    }

    /// Every key, the engine's settings too.
    pub fn delete_all() {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.values.clear();
            s.changes.push(Change::DeleteAll);
        });
    }

    /// Writes the file now (it is also written when the game closes).
    pub fn save() {
        STATE.with(|s| s.borrow_mut().changes.push(Change::Save));
    }
}
