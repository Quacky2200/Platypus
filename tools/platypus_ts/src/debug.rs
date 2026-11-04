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

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
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

/// A local variable at a stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    /// `number`, `string`, `boolean`, a class name, `array`.
    pub ty: String,
    pub value: String,
}

/// A frame of the call stack at a stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub name: String,
    /// A line of the bundled source.
    pub line: u32,
}

/// Where the game is stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    /// A line of the bundled source.
    pub line: u32,
    /// The stack pointer at the statement (stepping compares it; a call stack scan starts at it).
    pub sp: usize,
    pub reason: Reason,
    /// The named locals of the function it stopped in (empty when it has too many to spill).
    pub locals: Vec<Local>,
    /// The call stack, innermost first: the stop itself, then the callers found on the stack.
    /// Callers' locals are not kept.
    pub frames: Vec<Frame>,
}

// ---- what the generated code tells about its functions ----

/// How to show a local, from its static type.
#[derive(Clone, Debug)]
pub enum ShowTy {
    Num,
    Bool,
    /// A member of a string-literal union: an index into the module's tag texts.
    Lit(Arc<Vec<String>>),
    Str,
    Obj(String),
    Arr,
    Cell(Box<ShowTy>),
    Nullable(Box<ShowTy>),
    Other(String),
}

/// A function's locals for the debugger: (slot in the spilled frame, name, type).
#[derive(Clone, Debug)]
pub struct FuncDebug {
    pub name: String,
    pub locals: Vec<(usize, String, ShowTy)>,
}

static FUNCS: Mutex<Vec<(u32, Vec<FuncDebug>)>> = Mutex::new(Vec::new());
static NEXT_BASE: AtomicU32 = AtomicU32::new(1);

/// Reserves `count` function numbers for a module being compiled.
pub fn next_base(count: usize) -> u32 {
    NEXT_BASE.fetch_add(count as u32 + 1, Ordering::SeqCst)
}

/// Registers a module's functions (numbered from `base`). A reload registers its own range.
pub fn register(base: u32, funcs: Vec<FuncDebug>) {
    if let Ok(mut f) = FUNCS.lock() {
        f.push((base, funcs));
    }
}

fn func(id: u32) -> Option<FuncDebug> {
    let f = FUNCS.lock().ok()?;
    let (base, funcs) = f
        .iter()
        .rev()
        .find(|(b, fs)| id >= *b && ((id - *b) as usize) < fs.len())?;
    funcs.get((id - *base) as usize).cloned()
}

/// The compiled code of the running game, for the call stack.
struct Code {
    lines: Vec<(usize, usize, u32)>,
    /// (address, function name), ascending.
    funcs: Vec<(usize, String)>,
}

static CODE: Mutex<Option<Code>> = Mutex::new(None);

/// Gives the debugger the address tables of the code just compiled (replacing the old).
pub fn set_code(lines: Vec<(usize, usize, u32)>, mut funcs: Vec<(usize, String)>) {
    funcs.sort_unstable_by_key(|(a, _)| *a);
    if let Ok(mut c) = CODE.lock() {
        *c = Some(Code { lines, funcs });
    }
}

fn number(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.fract() == 0.0 && v.abs() < 9e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// The type name and text of a spilled local.
fn show(raw: u64, ty: &ShowTy) -> (String, String) {
    match ty {
        ShowTy::Num => ("number".into(), number(f64::from_bits(raw))),
        ShowTy::Bool => ("boolean".into(), (raw & 1 != 0).to_string()),
        ShowTy::Lit(tags) => (
            "string".into(),
            match tags.get(raw as u32 as usize) {
                Some(t) if raw as u32 != 0 => format!("{t:?}"),
                _ => "undefined".into(),
            },
        ),
        ShowTy::Str => ("string".into(), show_string(raw as usize)),
        ShowTy::Obj(name) => (
            name.clone(),
            if raw == 0 {
                "null".into()
            } else {
                format!("{name} @{raw:#x}")
            },
        ),
        ShowTy::Arr => (
            "array".into(),
            if raw == 0 {
                "null".into()
            } else {
                format!("Array @{raw:#x}")
            },
        ),
        ShowTy::Cell(inner) => {
            if raw == 0 {
                ("cell".into(), "null".into())
            } else {
                // A captured variable: the value follows the cell's header word.
                // SAFETY: a cell local holds a live cell (a header word, then the value).
                let v = unsafe { *((raw as usize + 8) as *const u64) };
                show(v, inner)
            }
        }
        ShowTy::Nullable(inner) => {
            let (t, v) = show(raw, inner);
            let null = match **inner {
                ShowTy::Num => f64::from_bits(raw).is_nan(),
                ShowTy::Lit(_) => raw as u32 == 0,
                _ => raw == 0,
            };
            (t, if null { "null".into() } else { v })
        }
        ShowTy::Other(t) => (t.clone(), format!("{raw:#x}")),
    }
}

fn show_string(p: usize) -> String {
    if p == 0 {
        return "null".into();
    }
    if !p.is_multiple_of(8) {
        return format!("<bad pointer {p:#x}>");
    }
    // SAFETY: a string local holds a live string or zero; a pointer that is not 8-aligned was
    // ruled out, and the length is checked before the bytes are read.
    let s = unsafe { platypus_ts_rt::Str::from_raw(p as *const platypus_ts_rt::Str) };
    if !(0..1 << 24).contains(&s.byte_len) {
        return format!("<string @{p:#x}>");
    }
    let t = String::from_utf8_lossy(s.as_bytes()).into_owned();
    let cut: String = t.chars().take(200).collect();
    let more = if cut.len() < t.len() { "…" } else { "" };
    format!("{cut:?}{more}")
}

/// The compiled-game frames above the stop, found by scanning the stack for return addresses
/// into compiled code (Windows; elsewhere the stop alone), from `from` upward. Stale words can show
/// as an extra frame.
#[cfg(windows)]
fn callers(from: usize, stop_line: u32, from_hook: bool) -> Vec<Frame> {
    let Ok(code) = CODE.lock() else {
        return Vec::new();
    };
    let Some(code) = code.as_ref() else {
        return Vec::new();
    };
    // SAFETY: `from` is an address in this thread's stack.
    let words = unsafe { crate::unwind::stack_words(from) };
    let mut out = Vec::new();
    let mut leading = true;
    for w in words {
        let at = w.wrapping_sub(1);
        let i = code.lines.partition_point(|(a, _, _)| *a <= at);
        let Some((a, b, line)) = i.checked_sub(1).map(|i| code.lines[i]) else {
            continue;
        };
        if at < a || at >= b {
            continue;
        }
        // Scanning from the hook's own frame (no spilled frame to start above): the leading
        // hits on the stop's own line are the hook call's return address and stale words of
        // the hook's frame, not callers.
        if from_hook && leading && line == stop_line {
            continue;
        }
        leading = false;
        let j = code.funcs.partition_point(|(f, _)| *f <= at);
        let name = j
            .checked_sub(1)
            .map(|j| code.funcs[j].1.clone())
            .unwrap_or_default();
        out.push(Frame { name, line });
    }
    out
}

#[cfg(not(windows))]
fn callers(_from: usize, _stop_line: u32, _from_hook: bool) -> Vec<Frame> {
    Vec::new()
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
    lock().stopped.clone()
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
    s.stopped.clone()
}

/// Lets a stopped game go on. Does nothing when it is not stopped.
pub fn resume(how: Resume) {
    let mut s = lock();
    let Some(at) = s.stopped.clone() else {
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

/// The hook: runs on the game's thread before a statement while the armed byte is set. `frame`
/// is the function's spilled locals (or null), `id` its descriptor (`register`).
extern "C" fn ts_dbg_line(line: u32, frame: *const u64, id: u32) {
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
    let desc = func(id);
    let locals = match (&desc, frame.is_null()) {
        (Some(d), false) => d
            .locals
            .iter()
            .map(|(slot, name, ty)| {
                // SAFETY: the generated code spilled every local of the function into `frame`.
                let (ty, value) = show(unsafe { *frame.add(*slot) }, ty);
                Local {
                    name: name.clone(),
                    ty,
                    value,
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut frames = vec![Frame {
        name: desc.map(|d| d.name).unwrap_or_default(),
        line,
    }];
    // Above the function's own spilled frame there are only its caller's words: the hook's
    // frame below it holds stale ones from earlier calls.
    frames.extend(if frame.is_null() {
        callers(sp, line, true)
    } else {
        callers(frame as usize, line, false)
    });
    s.stopped = Some(Stop {
        line,
        sp,
        reason,
        locals,
        frames,
    });
    s.mode = Mode::Run;
    s.go = false;
    rearm(&s);
    CHANGED.notify_all();
    while !s.go {
        s = CHANGED.wait(s).unwrap_or_else(|e| e.into_inner());
    }
    s.go = false;
}
