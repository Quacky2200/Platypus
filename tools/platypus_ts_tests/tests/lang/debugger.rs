// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The debug build's hook and the controller (`platypus_ts::debug`): breakpoints, pause,
//! continue and the three steps. The game runs on its own thread; the test is the debugger.
//! IDs are in TESTS.md (M9-TS-3393..3396).
use platypus_ts::debug::{self, Reason, Resume, Stop};
use platypus_ts::jit::{JitOptions, compile_and_load};
use std::time::Duration;

// Lines are those of this source (1-based).
const SRC: &str = "\
export function leaf(n: number): number {
  const a = n * 2;
  return a + 1;
}
export function run(n: number): number {
  let total = 0;
  total += leaf(n);
  total += leaf(n + 1);
  return total;
}
export function spin(): number {
  let i = 0;
  while (i < 100000000) {
    i += 1;
  }
  return i;
}
";

/// Compiles `SRC` as a debug build and runs `entry(arg)` on a thread; the result comes back
/// through `join`.
fn start(entry: &'static str, arg: f64, base: u32) -> std::thread::JoinHandle<f64> {
    std::thread::spawn(move || {
        let game = compile_and_load(
            SRC,
            &JitOptions {
                prefix: "dbgr_",
                struct_base: base,
                debug: true,
                ..Default::default()
            },
        )
        .unwrap();
        game.init().unwrap();
        let f: unsafe extern "C" fn(f64) -> f64 =
            unsafe { std::mem::transmute(game.function(entry).unwrap()) };
        unsafe { f(arg) }
    })
}

fn stop() -> Stop {
    debug::wait_stopped(Duration::from_secs(20)).expect("the game should have stopped")
}

// M9-TS-3393
#[test]
fn a_breakpoint_stops_the_game_at_its_line_and_continue_finishes_it() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    debug::set_breakpoints(&[7]);
    let game = start("run", 10.0, 2_370_000);
    let s = stop();
    assert_eq!((s.line, s.reason), (7, Reason::Breakpoint));
    debug::set_breakpoints(&[]);
    debug::resume(Resume::Continue);
    // leaf(10) + leaf(11) = 21 + 23
    assert_eq!(game.join().unwrap(), 44.0);
    assert!(debug::stopped().is_none());
}

// M9-TS-3394
#[test]
fn step_over_runs_a_call_through_and_step_in_enters_it() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    debug::set_breakpoints(&[7]);
    let game = start("run", 1.0, 2_371_000);
    assert_eq!(stop().line, 7);
    // Over the call: the next statement of `run`.
    debug::resume(Resume::StepOver);
    let s = stop();
    assert_eq!((s.line, s.reason), (8, Reason::Step));
    // Into the next call: its first statement.
    debug::resume(Resume::StepIn);
    assert_eq!(stop().line, 2);
    debug::resume(Resume::StepIn);
    assert_eq!(stop().line, 3);
    // Out: back in `run`, at the statement after the call.
    debug::resume(Resume::StepOut);
    assert_eq!(stop().line, 9);
    debug::detach();
    // leaf(1) + leaf(2) = 3 + 5
    assert_eq!(game.join().unwrap(), 8.0);
}

// M9-TS-3395
#[test]
fn pause_stops_a_running_loop_and_resuming_with_no_breakpoints_lets_it_end() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    // A breakpoint that is never reached arms the hook; the loop runs until paused.
    debug::set_breakpoints(&[999]);
    let game = start("spin", 0.0, 2_372_000);
    std::thread::sleep(Duration::from_millis(100));
    debug::pause();
    let s = stop();
    assert_eq!(s.reason, Reason::Pause);
    assert!((13..=15).contains(&s.line), "{s:?}");
    debug::detach();
    assert_eq!(game.join().unwrap(), 100_000_000.0);
}

// M9-TS-3396
#[test]
fn an_unarmed_debug_build_runs_without_stopping() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    let game = start("run", 3.0, 2_373_000);
    assert_eq!(game.join().unwrap(), 7.0 + 9.0);
    assert!(debug::stopped().is_none());
}
