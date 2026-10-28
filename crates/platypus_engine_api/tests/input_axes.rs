// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `input.getAxis` eases its keys like Unity's default axes; `getAxisRaw` does not (RDY-INP-01).
use platypus_engine_api::input::{self, input_api};
use platypus_engine_api::time;

fn frames(n: usize) {
    for _ in 0..n {
        time::advance(1.0 / 60.0);
    }
}

// RDY-INP-01
#[test]
fn keys_ease_at_three_per_second_and_raw_snaps() {
    input::reset();
    input::key_down_event("KeyD");
    frames(1);
    assert_eq!(input_api::get_axis_raw("Horizontal"), 1.0);
    assert!((input_api::get_axis("Horizontal") - 0.05).abs() < 1e-3);
    frames(9);
    assert!((input_api::get_axis("Horizontal") - 0.5).abs() < 1e-3);
    frames(30);
    assert_eq!(input_api::get_axis("Horizontal"), 1.0);
    // Released: back down at the same rate.
    input::key_up_event("KeyD");
    frames(10);
    assert!((input_api::get_axis("Horizontal") - 0.5).abs() < 1e-3);
    // The other way starts from zero, not through it.
    frames(60);
    input::key_down_event("KeyD");
    frames(5);
    input::key_up_event("KeyD");
    input::key_down_event("KeyA");
    frames(1);
    assert!((input_api::get_axis("Horizontal") + 0.05).abs() < 1e-3);
    // The mouse axes are not eased.
    assert_eq!(input_api::get_axis("Nope"), 0.0);
}
