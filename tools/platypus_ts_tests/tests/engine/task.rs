// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Task.delay`, `Task.delaySeconds` and `Task.frame` (`bench/task.ts`): awaitable waits on the
//! runtime timers, which the host advances with engine time. IDs are in TESTS.md (M9-TS-651..).
use platypus_ts_tests::task_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress, ts_rt_timers_advance};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_tsk_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn text() -> String {
    let p = unsafe { ts_tsk_text() };
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

// M9-TS-651
//
// A task waits on engine time: nothing happens before the delay is up, `delaySeconds` counts in
// seconds, and `frame` resolves at the next advance of the timers.
#[test]
fn tasks_wait_for_engine_time_and_for_the_next_frame() {
    in_every_gc_mode(|| {
        // The timers' clock only moves forward; each run starts later than the last ended.
        thread_local!(static RUNS: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) });
        let base = RUNS.with(|r| {
            r.set(r.get() + 1.0);
            r.get() * 1_000_000.0
        });
        ts_rt_timers_advance(base);
        unsafe { ts_tsk_start() };
        assert_eq!(text(), "a");
        ts_rt_timers_advance(base + 99.0);
        assert_eq!(text(), "a", "100 ms is not up yet");
        ts_rt_timers_advance(base + 100.0);
        assert_eq!(text(), "ab");
        ts_rt_timers_advance(base + 599.0);
        assert_eq!(text(), "ab", "half a second more is not up yet");
        ts_rt_timers_advance(base + 600.0);
        assert_eq!(text(), "abc");
        ts_rt_timers_advance(base + 616.0);
        assert_eq!(text(), "abcd", "the next frame resolves the frame task");
    });
}
