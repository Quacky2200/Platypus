// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The engine's current world for `#[ts_api]` functions (`ts_api::context`).
//! IDs are in TESTS.md (M9-API-*).
use std::cell::Cell;
use ts_api::{context, ts_api};

struct World {
    ticks: u32,
    names: Vec<&'static str>,
}

thread_local! {
    static WORLD: Cell<*mut World> = const { Cell::new(std::ptr::null_mut()) };
}

#[ts_api(namespace = "World")]
pub mod world_api {
    use super::{WORLD, context};

    /// Advances the world and returns the tick number, or 0 when no world is current.
    pub fn tick() -> u32 {
        context::with(&WORLD, |w| {
            w.ticks += 1;
            w.ticks
        })
        .unwrap_or(0)
    }

    pub fn object_count() -> u32 {
        context::with(&WORLD, |w| w.names.len() as u32).unwrap_or(0)
    }

    /// Calls `tick` while the world is borrowed: must not hand out a second `&mut`.
    pub fn tick_twice_nested() -> u32 {
        context::with(&WORLD, |_| tick()).unwrap_or(99)
    }
}

// M9-API-07
#[test]
fn shims_reach_the_world_the_host_made_current() {
    let mut world = World {
        ticks: 10,
        names: vec!["a", "b", "c"],
    };
    // No world yet: the functions answer with their defaults instead of crashing.
    assert_eq!(world_api::platypus_World_tick(), 0);
    {
        let _scope = context::enter(&WORLD, &mut world);
        assert_eq!(world_api::platypus_World_tick(), 11);
        assert_eq!(world_api::platypus_World_tick(), 12);
        assert_eq!(world_api::platypus_World_objectCount(), 3);
    }
    // Leaving the scope removes it again, and the world kept the changes.
    assert_eq!(world_api::platypus_World_tick(), 0);
    assert_eq!(world.ticks, 12);
}

// M9-API-08
#[test]
fn a_nested_call_does_not_get_a_second_mutable_reference() {
    let mut world = World {
        ticks: 0,
        names: vec![],
    };
    let _scope = context::enter(&WORLD, &mut world);
    // Inside `with`, the world is not current: the inner `tick` finds none and returns 0.
    assert_eq!(world_api::platypus_World_tickTwiceNested(), 0);
    // And it is current again afterwards.
    assert_eq!(world_api::platypus_World_tick(), 1);
}

// M9-API-09
#[test]
fn scopes_nest_and_restore_the_previous_world() {
    let mut outer = World {
        ticks: 100,
        names: vec![],
    };
    let mut inner = World {
        ticks: 200,
        names: vec![],
    };
    let _a = context::enter(&WORLD, &mut outer);
    assert_eq!(world_api::platypus_World_tick(), 101);
    {
        let _b = context::enter(&WORLD, &mut inner);
        assert_eq!(world_api::platypus_World_tick(), 201);
    }
    assert_eq!(world_api::platypus_World_tick(), 102);
}
