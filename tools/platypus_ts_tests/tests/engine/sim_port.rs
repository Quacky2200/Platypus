// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The 13 cases of `fixtures/sim.test.ts`, run against native code that
//! `web_ui_ts` compiled from `sim.ts`. Test IDs are in TESTS.md (M9-TS-*).
#![allow(non_snake_case)]

use platypus_ts_tests::sim::*;
use platypus_ts_tests::*;
use web_ui_rt::{Closure, link_anchor, ts_rt_arr_new};

const W: usize = 1022;
const H: usize = 750;

struct World {
    open: *mut Closure,
    solid: *mut Closure,
    idle: *mut PlayerInput,
}

fn world() -> World {
    assert_ne!(link_anchor(), 0);
    gc_mode_from_env();
    unsafe { ts_init() };
    let open = build_walls(W, H, &vec![vec![W as f64]; H]);
    let solid = build_walls(W, H, &vec![vec![0.0, W as f64]; H]);
    // Aim straight right of the player centre (the centre is at y 375), so the angle is 0.
    World {
        open: walls_closure(open),
        solid: walls_closure(solid),
        idle: new_input(0.0, 0.0, 900.0, 375.0, false),
    }
}

fn solo_state(w: &World) -> (*mut SimState, *mut Player) {
    unsafe {
        let state = root(ts_createState(7.0, 6.0));
        let p = ts_addPlayer(state);
        ts_startLevel(state, w.open);
        (state, p)
    }
}

fn first_player(state: *mut SimState) -> *mut Player {
    unsafe { (*(*state).players).get::<*mut Player>(0) }
}

fn step_once(state: *mut SimState, input: *mut PlayerInput, walls: *mut Closure) {
    unsafe { ts_step(state, inputs(&[input]), 1.0 / 60.0, walls) };
}

// M9-TS-01
#[test]
fn a_baked_mask_keeps_the_runs_of_walls_and_open_floor() {
    let _serial = platypus_ts_tests::serial();
    let _ = world();
    let walls = build_walls(4, 2, &[vec![1.0, 1.0, 2.0], vec![0.0, 4.0]]);
    let cells = unsafe { (*(*walls).cells).bytes().to_vec() };
    assert_eq!(&cells[..5], &[0, 1, 0, 0, 1]);
}

// M9-TS-02
#[test]
fn zombies_spawn_in_single_player() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, _) = solo_state(&w);
    run(state, 0.5, inputs(&[w.idle]), w.open);
    assert!(arr_len(unsafe { (*state).zombies }) > 0);
}

// M9-TS-03
#[test]
fn a_levels_zombie_budget_grows_by_3_to_5_each_level() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, _) = solo_state(&w);
    let max = unsafe { (*state).zombiesMax };
    assert!((8.0..=10.0).contains(&max), "{max}");
}

// M9-TS-04
#[test]
fn a_player_moves_400_px_a_second_with_one_key_held() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    let x0 = unsafe { (*p).x };
    let right = new_input(1.0, 0.0, 900.0, 375.0, false);
    run(state, 0.5, inputs(&[right]), w.open);
    let dx = unsafe { (*p).x } - x0;
    assert!(dx > 195.0 && dx < 205.0, "{dx}");
}

// M9-TS-05
#[test]
fn a_wall_stops_the_player() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    let x0 = unsafe { (*p).x };
    let right = new_input(1.0, 0.0, 900.0, 375.0, false);
    run(state, 0.5, inputs(&[right]), w.solid);
    assert_eq!(unsafe { (*p).x }, x0);
}

// M9-TS-06
#[test]
fn a_press_fires_one_pistol_round_and_turns_the_aim_toward_the_mouse() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    let (before, fire) = (
        unsafe { (*(*p).gun).rounds },
        new_input(0.0, 0.0, 900.0, 375.0, true),
    );
    step_once(state, fire, w.open);
    unsafe {
        assert_eq!((*(*p).gun).rounds, before - 1.0);
        assert_eq!(arr_len((*state).bullets), 1);
        assert!((*p).angle.abs() < 0.5);
    }
}

// M9-TS-07
#[test]
fn holding_fire_with_the_pistol_does_not_repeat() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    let before = unsafe { (*(*p).gun).rounds };
    run(
        state,
        1.0,
        inputs(&[new_input(0.0, 0.0, 900.0, 375.0, true)]),
        w.open,
    );
    assert_eq!(unsafe { (*(*p).gun).rounds }, before - 1.0);
}

// M9-TS-08
#[test]
fn an_empty_clip_starts_a_reload_that_blocks_shooting() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    unsafe {
        (*(*p).gun).rounds = 16.0;
        (*p).bulletsInClip = 1.0;
    }
    step_once(state, new_input(0.0, 0.0, 900.0, 375.0, true), w.open);
    unsafe {
        assert_eq!((*(*p).gun).rounds, 15.0);
        assert!((*p).reloadTimer > 0.0);
        let ev = (*state).events;
        let kinds: Vec<u64> = (0..arr_len(ev))
            .map(|i| (*(*ev).get::<*mut SimEvent>(i)).kind)
            .collect();
        assert!(kinds.contains(&(TAG_reload as u64)), "{kinds:?}");
    }
}

// M9-TS-09
#[test]
fn a_hit_takes_health_and_scores_10_a_kill_scores_80_once_the_fade_ends() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    let z = new_zombie(999.0, 600.0, 300.0, 10.0, None, unsafe { (*p).id });
    push_ptr(unsafe { (*state).zombies }, z);
    unsafe {
        ts_damageZombie(state, z, 4.0, p);
        assert_eq!((*z).hp, 6.0);
        assert_eq!((*p).score, 10.0);
        ts_damageZombie(state, z, 6.0, p);
        assert_eq!((*z).dying, 1);
    }
    run(state, 0.5, inputs(&[w.idle]), w.open);
    unsafe {
        assert_eq!((*p).kills, 1.0);
        assert_eq!((*p).score, 10.0 + 10.0 + 80.0);
    }
}

// M9-TS-10
#[test]
fn four_zombie_hits_kill_a_player_with_100_health_and_the_score_is_floored_at_0() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    unsafe {
        (*p).score = 50.0;
        let z = new_zombie(999.0, (*p).x, (*p).y, 10.0, Some(0.0), (*p).id);
        push_ptr((*state).zombies, z);
    }
    run(state, 3.0, inputs(&[w.idle]), w.open);
    unsafe {
        assert_eq!((*p).dead, 1);
        assert_eq!((*p).score, 0.0);
        assert_eq!((*state).gameOver, 1);
        assert_eq!(arr_len((*state).events), 0);
    }
}

// M9-TS-11
#[test]
fn the_next_level_starts_when_the_last_zombie_of_this_one_dies() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    let z = new_zombie(999.0, 600.0, 300.0, 10.0, None, unsafe { (*p).id });
    unsafe {
        (*state).zombiesLeft = 0.0;
        (*state).zombies = ts_rt_arr_new(0, 1);
        push_ptr((*state).zombies, z);
        (*state).levelEnding = 0;
        ts_damageZombie(state, z, 10.0, p);
    }
    run(state, 0.5, inputs(&[w.idle]), w.open);
    assert_eq!(unsafe { (*state).level }, 2.0);
}

// M9-TS-12
#[test]
fn touching_an_ammo_powerup_adds_320_rounds() {
    let _serial = platypus_ts_tests::serial();
    let w = world();
    let (state, p) = solo_state(&w);
    let before = unsafe { (*(*p).gun).rounds };
    unsafe {
        let pw = alloc::<Powerup>(HDR_Powerup);
        (*pw).id = 1000.0;
        (*pw).r#type = TAG_ammo as u64;
        (*pw).x = (*p).x;
        (*pw).y = (*p).y;
        push_ptr((*state).powerups, pw);
    }
    step_once(state, w.idle, w.open);
    unsafe {
        assert_eq!((*(*p).gun).rounds, before + 320.0);
        assert_eq!(arr_len((*state).powerups), 0);
    }
    assert_eq!(first_player(state), p);
}

// M9-TS-13
#[test]
fn zombies_speed_up_a_little_each_level_until_level_3_then_hold() {
    let _serial = platypus_ts_tests::serial();
    let _ = world();
    let close = |a: f64, b: f64| (a - b).abs() < 0.005;
    unsafe {
        assert_eq!(ts_zombieSpeed(1.0), 1.0);
        assert!(close(ts_zombieSpeed(2.0), 1.15));
        assert!(close(ts_zombieSpeed(3.0), 1.3));
        assert!(close(ts_zombieSpeed(9.0), 1.3));
    }
}
