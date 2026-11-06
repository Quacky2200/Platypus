// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The debug build's hook and the controller (`web_ui_ts::debug`): breakpoints, pause,
//! continue and the three steps. The game runs on its own thread; the test is the debugger.
//! IDs are in TESTS.md (M9-TS-3393..3396, 3400..3407, 3412).
use std::time::Duration;
use web_ui_ts::debug::{self, BreakpointSpec, Reason, Resume, Stop};
use web_ui_ts::jit::{JitOptions, compile_and_load};

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

// Lines are those of this source (1-based).
const SRC_LOCALS: &str = "\
export function labels(n: number): number {
  const name = 'hi ' + n;
  const flag = n > 1;
  let count = 0;
  const bump = () => { count += 1; };
  bump();
  count += name.length;
  return flag ? count : -count;
}
export function outer(n: number): number {
  return labels(n) + 1;
}
";

fn start_src(
    src: &'static str,
    entry: &'static str,
    arg: f64,
    base: u32,
) -> std::thread::JoinHandle<f64> {
    std::thread::spawn(move || {
        let game = compile_and_load(
            src,
            &JitOptions {
                prefix: "dbgl_",
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

// M9-TS-3400
#[test]
fn a_stop_shows_the_functions_named_locals_with_their_types() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    debug::set_breakpoints(&[7]);
    let game = start_src(SRC_LOCALS, "labels", 2.0, 2_374_000);
    let s = stop();
    let find = |name: &str| {
        s.locals
            .iter()
            .find(|l| l.name == name)
            .unwrap_or_else(|| panic!("no local {name} in {:?}", s.locals))
            .clone()
    };
    let n = find("n");
    assert_eq!((n.ty.as_str(), n.value.as_str()), ("number", "2"));
    let name = find("name");
    assert_eq!(
        (name.ty.as_str(), name.value.as_str()),
        ("string", "\"hi 2\"")
    );
    let flag = find("flag");
    assert_eq!((flag.ty.as_str(), flag.value.as_str()), ("boolean", "true"));
    // A variable a closure captured lives in a cell: its value is shown, not the cell.
    let count = find("count");
    assert_eq!((count.ty.as_str(), count.value.as_str()), ("number", "1"));
    debug::detach();
    // flag is true: count is 1 + "hi 2".length
    assert_eq!(game.join().unwrap(), 5.0);
}

// M9-TS-3401
#[test]
fn a_stop_lists_the_callers_on_the_stack() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    debug::set_breakpoints(&[7]);
    let game = start_src(SRC_LOCALS, "outer", 2.0, 2_375_000);
    let s = stop();
    assert_eq!((s.frames[0].name.as_str(), s.frames[0].line), ("labels", 7));
    // The scan finds the caller where there is a stack walk (Windows).
    #[cfg(windows)]
    {
        let names: Vec<&str> = s.frames.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(&names[..2], ["labels", "outer"], "{:?}", s.frames);
        assert_eq!(s.frames[1].line, 11);
    }
    debug::detach();
    assert_eq!(game.join().unwrap(), 6.0);
}

// Lines are those of this source (1-based).
const SRC_VALUES: &str = "\
interface P { x: number; label: string }
export function shapes(n: number): number {
  const p: P = { x: n, label: 'pt' };
  const arr: number[] = [1, 2, 3];
  const cnt = arr.length + p.x;
  return cnt;
}
export function looped(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    total += i;
  }
  return total;
}
";

fn start_values(entry: &'static str, arg: f64, base: u32) -> std::thread::JoinHandle<f64> {
    start_src(SRC_VALUES, entry, arg, base)
}

fn eval(text: &str) -> (String, String) {
    let v = debug::evaluate(text).unwrap_or_else(|e| panic!("{text}: {e}"));
    (v.ty, v.value)
}

// M9-TS-3402
#[test]
fn watch_expressions_read_locals_fields_elements_and_do_arithmetic() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    debug::set_breakpoints(&[5]);
    let game = start_values("shapes", 2.0, 2_376_000);
    stop();
    assert_eq!(eval("n"), ("number".into(), "2".into()));
    assert_eq!(eval("p.x"), ("number".into(), "2".into()));
    assert_eq!(eval("p.label"), ("string".into(), "\"pt\"".into()));
    assert_eq!(eval("arr.length"), ("number".into(), "3".into()));
    assert_eq!(eval("arr[1]"), ("number".into(), "2".into()));
    assert_eq!(eval("p.x * 2 + arr[2]").1, "7");
    assert_eq!(eval("p.x > 1 && arr.length == 3").1, "true");
    assert_eq!(eval("p.label + '!' + n").1, "\"pt!2\"");
    assert_eq!(eval("arr[9]").1, "undefined");
    // Errors name the problem.
    assert!(debug::evaluate("nope").unwrap_err().contains("not defined"));
    assert!(debug::evaluate("p.nope").unwrap_err().contains("no field"));
    assert!(debug::evaluate("1 +").is_err());
    debug::detach();
    assert_eq!(game.join().unwrap(), 5.0);
}

// M9-TS-3403
#[test]
fn objects_and_arrays_open_into_their_fields_and_elements() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    debug::set_breakpoints(&[5]);
    let game = start_values("shapes", 2.0, 2_377_000);
    stop();
    let vars = debug::scope_variables();
    let find = |name: &str| vars.iter().find(|v| v.name == name).unwrap().clone();
    let p = find("p");
    assert_eq!(p.value, "P");
    assert!(p.handle >= 2, "{p:?}");
    let fields: Vec<(String, String)> = debug::variables(p.handle)
        .into_iter()
        .map(|v| (v.name, v.value))
        .collect();
    assert_eq!(
        fields,
        [
            ("x".to_string(), "2".to_string()),
            ("label".to_string(), "\"pt\"".to_string())
        ]
    );
    let arr = find("arr");
    assert_eq!(arr.value, "Array(3)");
    let items: Vec<String> = debug::variables(arr.handle)
        .into_iter()
        .map(|v| format!("{}={}", v.name, v.value))
        .collect();
    assert_eq!(items, ["[0]=1", "[1]=2", "[2]=3"]);
    // A number has nothing inside.
    assert_eq!(find("n").handle, 0);
    debug::detach();
    assert_eq!(game.join().unwrap(), 5.0);
}

fn run_with(
    specs: &[BreakpointSpec],
    entry: &'static str,
    arg: f64,
    base: u32,
) -> std::thread::JoinHandle<f64> {
    debug::detach();
    debug::set_breakpoint_specs(specs)
        .into_iter()
        .for_each(|r| r.unwrap());
    start_values(entry, arg, base)
}

// M9-TS-3404
#[test]
fn a_conditional_breakpoint_stops_only_when_its_condition_is_true() {
    let _serial = platypus_ts_tests::serial();
    let game = run_with(
        &[BreakpointSpec {
            line: 11,
            condition: Some("i == 3".into()),
            ..BreakpointSpec::default()
        }],
        "looped",
        6.0,
        2_378_000,
    );
    stop();
    assert_eq!(eval("i").1, "3");
    assert_eq!(eval("total").1, "3");
    debug::detach();
    assert_eq!(game.join().unwrap(), 15.0);
}

// M9-TS-3405
#[test]
fn a_hit_condition_counts_the_hits_that_pass_the_condition() {
    let _serial = platypus_ts_tests::serial();
    let game = run_with(
        &[BreakpointSpec {
            line: 11,
            hit_condition: Some("3".into()),
            ..BreakpointSpec::default()
        }],
        "looped",
        6.0,
        2_379_000,
    );
    stop();
    // The third time through the loop body.
    assert_eq!(eval("i").1, "2");
    debug::resume(Resume::Continue);
    // The hit condition `3` is exactly the third hit: the game now runs to the end.
    assert_eq!(game.join().unwrap(), 15.0);
    assert!(debug::stopped().is_none());
    // `% 2` stops on every second hit.
    let game = run_with(
        &[BreakpointSpec {
            line: 11,
            hit_condition: Some("% 2".into()),
            ..BreakpointSpec::default()
        }],
        "looped",
        6.0,
        2_380_000,
    );
    stop();
    assert_eq!(eval("i").1, "1");
    debug::resume(Resume::Continue);
    stop();
    assert_eq!(eval("i").1, "3");
    debug::detach();
    game.join().unwrap();
}

// M9-TS-3406
#[test]
fn a_log_point_writes_its_message_with_values_and_does_not_stop() {
    let _serial = platypus_ts_tests::serial();
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    debug::set_output(Some(Box::new(move |m| {
        sink.lock().unwrap().push(m.to_string())
    })));
    let game = run_with(
        &[BreakpointSpec {
            line: 11,
            log_message: Some("i={i} total={total} big={i > 1}".into()),
            ..BreakpointSpec::default()
        }],
        "looped",
        4.0,
        2_381_000,
    );
    assert_eq!(game.join().unwrap(), 6.0);
    debug::set_output(None);
    debug::detach();
    assert_eq!(
        *lines.lock().unwrap(),
        [
            "i=0 total=0 big=false",
            "i=1 total=0 big=false",
            "i=2 total=1 big=true",
            "i=3 total=3 big=true"
        ]
    );
}

// M9-TS-3407
#[test]
fn a_condition_or_hit_condition_that_does_not_parse_is_reported_and_not_set() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    let r = debug::set_breakpoint_specs(&[
        BreakpointSpec {
            line: 3,
            condition: Some("a +".into()),
            ..BreakpointSpec::default()
        },
        BreakpointSpec {
            line: 4,
            hit_condition: Some("many".into()),
            ..BreakpointSpec::default()
        },
        BreakpointSpec {
            line: 5,
            condition: Some("n > 1".into()),
            ..BreakpointSpec::default()
        },
    ]);
    assert!(r[0].is_err() && r[1].is_err() && r[2].is_ok(), "{r:?}");
    debug::detach();
}

// Lines are those of this source (1-based).
const SRC_THROW: &str = "\
export function thrower(n: number): number {
  let seen = 0;
  try {
    seen = n;
    if (n > 1) throw new Error('big');
  } catch (e) {
    return -seen;
  }
  return n;
}
";

// M9-TS-3412
#[test]
fn break_on_throw_stops_at_the_throw_statement_and_off_it_does_not() {
    let _serial = platypus_ts_tests::serial();
    debug::detach();
    // Off: the exception is caught and nothing stops.
    let game = start_src(SRC_THROW, "thrower", 5.0, 2_382_000);
    assert_eq!(game.join().unwrap(), -5.0);
    assert!(debug::stopped().is_none());
    // On: it stops where the error is thrown, with the locals at that point.
    debug::set_break_on_throw(true);
    let game = start_src(SRC_THROW, "thrower", 5.0, 2_383_000);
    let s = stop();
    assert_eq!((s.reason, s.line), (Reason::Exception, 5));
    assert_eq!(eval("seen").1, "5");
    assert_eq!(eval("n > 1").1, "true");
    debug::resume(Resume::Continue);
    assert_eq!(game.join().unwrap(), -5.0);
    // A call that does not throw is not stopped by it.
    let game = start_src(SRC_THROW, "thrower", 1.0, 2_384_000);
    assert_eq!(game.join().unwrap(), 1.0);
    debug::detach();
}
