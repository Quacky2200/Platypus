// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! platypus_ts tests: scenes, GameObjects, components, physics and contacts, animation, the zombie
//! sim and the GC over it. One of five test binaries by area, so an edit relinks a smaller one.

mod anim_animator;
mod anim_clips;
mod anim_player;
mod comp_api;
mod comp_life;
mod comp_query;
mod components;
mod components2;
mod contacts;
mod engine;
mod engine2;
mod gc;
mod physics;
mod physics_comp;
mod scene;
mod scene_objects;
mod scenes;
mod sim_port;
mod sine_demo_full;
mod task;

// The `ts_rt_component_*` functions the linked programs call.
use platypus_components as _;
