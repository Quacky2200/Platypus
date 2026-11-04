// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The debugger's side of debug builds (`JitOptions::debug`, design in
//! `docs/research/platypus-ts-debug-info.md`).
//!
//! A debug build puts a check before every statement: when the armed byte is non-zero it calls
//! `ts_dbg_line(line)` with the line of the bundled source. The hook decides whether the game
//! stops there (a breakpoint, a pause request, or the step being taken) and, if so, parks the
//! game's thread until a controller on another thread resumes it. Everything here is
//! process-wide: one game runs in a process.
//!
//! Stepping compares the stack pointer, so no call depth has to be tracked: a statement of the
//! same function has the same stack pointer, a callee a lower one and a caller a higher one.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Non-zero while any breakpoint, pause request or step is set: the generated check reads this
/// byte, so a game that nobody debugs pays a load and a branch per statement.
static ARMED: AtomicU8 = AtomicU8::new(0);

/// The address of the armed byte, for the generated code.
pub fn armed_addr() -> usize {
    std::ptr::addr_of!(ARMED) as usize
}

/// The address of the hook the generated code calls.
pub fn hook_addr() -> usize {
    ts_dbg_line as *const () as usize
}

/// How the game goes on after a stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resume {
    /// Until a breakpoint or a pause request.
    Continue,
    /// To the next statement, into calls.
    StepIn,
    /// To the next statement of this function (or a caller's): calls run through.
    StepOver,
    /// Until the function returns to its caller.
    StepOut,
}

/// Why the game stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Breakpoint,
    Pause,
    Step,
}

/// Where the game is stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    /// A line of the bundled source.
    pub line: u32,
    /// The stack pointer at the statement (stepping compares it; a call stack scan starts at it).
    pub sp: usize,
    pub reason: Reason,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    Pause,
    In,
    Over(usize),
    Out(usize),
}

struct State {
    /// Sorted lines of the bundled source.
    breakpoints: Vec<u32>,
    mode: Mode,
    stopped: Option<Stop>,
    /// Set by the controller to let the parked thread go.
    go: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    breakpoints: Vec::new(),
    mode: Mode::Run,
    stopped: None,
    go: false,
});
/// Signalled when the game stops and when it is let go.
static CHANGED: Condvar = Condvar::new();

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn rearm(s: &State) {
    let on = !s.breakpoints.is_empty() || s.mode != Mode::Run;
    ARMED.store(u8::from(on), Ordering::SeqCst);
}

/// Replaces the breakpoints (lines of the bundled source).
pub fn set_breakpoints(lines: &[u32]) {
    let mut s = lock();
    s.breakpoints = lines.to_vec();
    s.breakpoints.sort_unstable();
    s.breakpoints.dedup();
    rearm(&s);
}

/// Asks the game to stop at the next statement.
pub fn pause() {
    let mut s = lock();
    if s.stopped.is_none() {
        s.mode = Mode::Pause;
    }
    rearm(&s);
}

/// Where the game is stopped, if it is.
pub fn stopped() -> Option<Stop> {
    lock().stopped
}

/// Waits until the game stops (at most `timeout`).
pub fn wait_stopped(timeout: Duration) -> Option<Stop> {
    let end = Instant::now() + timeout;
    let mut s = lock();
    while s.stopped.is_none() {
        let left = end.checked_duration_since(Instant::now())?;
        s = CHANGED
            .wait_timeout(s, left)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    s.stopped
}

/// Lets a stopped game go on. Does nothing when it is not stopped.
pub fn resume(how: Resume) {
    let mut s = lock();
    let Some(at) = s.stopped else {
        return;
    };
    s.mode = match how {
        Resume::Continue => Mode::Run,
        Resume::StepIn => Mode::In,
        Resume::StepOver => Mode::Over(at.sp),
        Resume::StepOut => Mode::Out(at.sp),
    };
    s.stopped = None;
    s.go = true;
    rearm(&s);
    CHANGED.notify_all();
}

/// Forgets every breakpoint and request, and lets a stopped game go (a debugger detaching).
pub fn detach() {
    let mut s = lock();
    s.breakpoints.clear();
    s.mode = Mode::Run;
    s.stopped = None;
    s.go = true;
    rearm(&s);
    CHANGED.notify_all();
}

/// The hook: runs on the game's thread before a statement while the armed byte is set.
extern "C" fn ts_dbg_line(line: u32) {
    // A statement's stack pointer, near enough: the address of a local of this frame.
    let marker = 0u8;
    let sp = std::ptr::addr_of!(marker) as usize;
    let mut s = lock();
    let reason = match s.mode {
        _ if s.stopped.is_some() => return,
        Mode::Pause => Some(Reason::Pause),
        Mode::In => Some(Reason::Step),
        Mode::Over(at) if sp >= at => Some(Reason::Step),
        Mode::Out(at) if sp > at => Some(Reason::Step),
        _ => None,
    };
    let reason = reason.or_else(|| {
        s.breakpoints
            .binary_search(&line)
            .is_ok()
            .then_some(Reason::Breakpoint)
    });
    let Some(reason) = reason else {
        return;
    };
    s.stopped = Some(Stop { line, sp, reason });
    s.mode = Mode::Run;
    s.go = false;
    rearm(&s);
    CHANGED.notify_all();
    while !s.go {
        s = CHANGED.wait(s).unwrap_or_else(|e| e.into_inner());
    }
    s.go = false;
}
