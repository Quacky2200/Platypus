// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The debugger's side of debug builds (`JitOptions::debug`, design in
//! `docs/research/platypus-ts-debug-info.md`).
//!
//! A debug build puts a check before every statement: when the armed byte is non-zero it calls
//! `ts_dbg_line(line, frame, function)` with the line of the bundled source, the function's
//! spilled locals and its number. The hook decides whether the game stops there (a breakpoint
//! whose condition and hit count say so, a pause request, or the step being taken) and, if so,
//! parks the game's thread until a controller on another thread resumes it. While it is parked
//! the controller reads variables and evaluates expressions (`debug_eval.rs`) straight from the
//! game's memory. Everything here is process-wide: one game runs in a process.
//!
//! Stepping compares the stack pointer, so no call depth has to be tracked: a statement of the
//! same function has the same stack pointer, a callee a lower one and a caller a higher one.

use crate::debug_eval::{self, Expr, HitCondition, Scope, Val};
pub use crate::debug_eval::{ShowTy, StructInfo, Structs};
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

/// A variable or evaluated expression as a debugger lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub ty: String,
    pub value: String,
    /// 0 for a value with nothing inside; otherwise what [`variables`] opens (2 and up).
    pub handle: u32,
}

// ---- what the generated code tells about its functions ----

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

// ---- breakpoints and the stop state ----

/// A breakpoint as a debugger gives it: a line of the bundled source and what makes it stop.
#[derive(Clone, Debug, Default)]
pub struct BreakpointSpec {
    pub line: u32,
    /// An expression over the function's locals: stop only when it is true.
    pub condition: Option<String>,
    /// `5`, `>= 5`, `% 2`..: stop only on those hits (counted after the condition).
    pub hit_condition: Option<String>,
    /// Log this (`{expression}` is replaced by its value) instead of stopping.
    pub log_message: Option<String>,
}

struct Spec {
    line: u32,
    cond: Option<Expr>,
    hit: Option<HitCondition>,
    log: Option<String>,
    hits: u64,
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
    breakpoints: Vec<Spec>,
    mode: Mode,
    stopped: Option<Stop>,
    /// Set by the controller to let the parked thread go.
    go: bool,
    /// The stopped function's named locals, raw: what expressions read.
    scope: Option<Arc<Scope>>,
    /// The values a debugger was given a handle to (handle = index + 2); forgotten on resume.
    handles: Vec<Val>,
}

static STATE: Mutex<State> = Mutex::new(State {
    breakpoints: Vec::new(),
    mode: Mode::Run,
    stopped: None,
    go: false,
    scope: None,
    handles: Vec::new(),
});
/// Signalled when the game stops and when it is let go.
static CHANGED: Condvar = Condvar::new();

/// Where log points write (the controller's output).
type Output = Box<dyn Fn(&str) + Send>;
static OUTPUT: Mutex<Option<Output>> = Mutex::new(None);

/// Called while the game is parked, a few times a second: a host keeps its window alive with it.
type Idle = Box<dyn Fn() + Send>;
static IDLE: Mutex<Option<Idle>> = Mutex::new(None);

/// Sets where log points write; `None` drops them.
pub fn set_output(f: Option<Output>) {
    if let Ok(mut o) = OUTPUT.lock() {
        *o = f;
    }
}

/// Sets what the parked game thread does while it waits, about every 30 ms (it runs on the game's
/// thread: pumping the window's messages is its use).
pub fn set_idle(f: Option<Idle>) {
    if let Ok(mut i) = IDLE.lock() {
        *i = f;
    }
}

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn rearm(s: &State) {
    let on = !s.breakpoints.is_empty() || s.mode != Mode::Run;
    ARMED.store(u8::from(on), Ordering::SeqCst);
}

/// Replaces the breakpoints. Returns, per spec, an error for a condition or hit condition that
/// does not parse (that breakpoint is not set).
pub fn set_breakpoint_specs(specs: &[BreakpointSpec]) -> Vec<Result<(), String>> {
    let mut s = lock();
    s.breakpoints.clear();
    let mut results = Vec::new();
    for spec in specs {
        let cond = spec.condition.as_deref().filter(|c| !c.trim().is_empty());
        let hit = spec
            .hit_condition
            .as_deref()
            .filter(|c| !c.trim().is_empty());
        let parsed = cond
            .map(debug_eval::parse)
            .transpose()
            .map_err(|e| format!("condition: {e}"))
            .and_then(|c| hit.map(HitCondition::parse).transpose().map(|h| (c, h)));
        match parsed {
            Ok((cond, hit)) => {
                s.breakpoints.push(Spec {
                    line: spec.line,
                    cond,
                    hit,
                    log: spec.log_message.clone().filter(|m| !m.is_empty()),
                    hits: 0,
                });
                results.push(Ok(()));
            }
            Err(e) => results.push(Err(e)),
        }
    }
    s.breakpoints.sort_by_key(|b| b.line);
    rearm(&s);
    results
}

/// Replaces the breakpoints with plain ones (lines of the bundled source).
pub fn set_breakpoints(lines: &[u32]) {
    let specs: Vec<BreakpointSpec> = lines
        .iter()
        .map(|l| BreakpointSpec {
            line: *l,
            ..BreakpointSpec::default()
        })
        .collect();
    let _ = set_breakpoint_specs(&specs);
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
    s.scope = None;
    s.handles.clear();
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
    s.scope = None;
    s.handles.clear();
    s.go = true;
    rearm(&s);
    CHANGED.notify_all();
}

// ---- reading a stopped game ----

/// Lists a value for a debugger, giving it a handle when it has parts to open.
fn variable(s: &mut State, name: String, v: Val) -> Variable {
    let (ty, value) = debug_eval::describe(&v);
    let handle = if debug_eval::children(&v).is_empty() {
        0
    } else {
        s.handles.push(v);
        s.handles.len() as u32 + 1
    };
    Variable {
        name,
        ty,
        value,
        handle,
    }
}

/// The innermost frame's named locals.
pub fn scope_variables() -> Vec<Variable> {
    let mut s = lock();
    let Some(scope) = s.scope.clone() else {
        return Vec::new();
    };
    scope
        .locals
        .iter()
        .map(|(name, raw, ty)| variable(&mut s, name.clone(), debug_eval::from_raw(*raw, ty)))
        .collect()
}

/// The parts of a value a handle names (1 is the locals).
pub fn variables(handle: u32) -> Vec<Variable> {
    if handle == 1 {
        return scope_variables();
    }
    let mut s = lock();
    let Some(v) = handle
        .checked_sub(2)
        .and_then(|i| s.handles.get(i as usize))
        .cloned()
    else {
        return Vec::new();
    };
    debug_eval::children(&v)
        .into_iter()
        .map(|(name, v)| variable(&mut s, name, v))
        .collect()
}

/// Evaluates an expression in the stopped function's scope (a watch).
pub fn evaluate(text: &str) -> Result<Variable, String> {
    let mut s = lock();
    let Some(scope) = s.scope.clone() else {
        return Err("the game is not stopped".into());
    };
    let e = debug_eval::parse(text)?;
    let v = debug_eval::eval(&e, &scope)?;
    Ok(variable(&mut s, text.to_string(), v))
}

fn scope_of(desc: &Option<FuncDebug>, frame: *const u64) -> Scope {
    match desc {
        Some(d) if !frame.is_null() => Scope {
            locals: d
                .locals
                .iter()
                .map(|(slot, name, ty)| {
                    // SAFETY: the generated code spilled every local of the function into `frame`.
                    (name.clone(), unsafe { *frame.add(*slot) }, ty.clone())
                })
                .collect(),
        },
        _ => Scope::default(),
    }
}

/// The hook: runs on the game's thread before a statement while the armed byte is set. `frame`
/// is the function's spilled locals (or null), `id` its descriptor (`register`).
extern "C" fn ts_dbg_line(line: u32, frame: *const u64, id: u32) {
    // A statement's stack pointer, near enough: the address of a local of this frame.
    let marker = 0u8;
    let sp = std::ptr::addr_of!(marker) as usize;
    let mut s = lock();
    if s.stopped.is_some() {
        return;
    }
    let step = match s.mode {
        Mode::Pause => Some(Reason::Pause),
        Mode::In => Some(Reason::Step),
        Mode::Over(at) if sp >= at => Some(Reason::Step),
        Mode::Out(at) if sp > at => Some(Reason::Step),
        _ => None,
    };
    let desc = func(id);
    let mut scope = None;
    let mut at_breakpoint = false;
    if s.breakpoints.iter().any(|b| b.line == line) {
        let sc = scope.insert(scope_of(&desc, frame));
        for b in s.breakpoints.iter_mut().filter(|b| b.line == line) {
            // A condition that cannot be evaluated stops: the debugger should see why.
            if let Some(c) = &b.cond
                && let Ok(v) = debug_eval::eval(c, sc)
                && !debug_eval::truthy(&v)
            {
                continue;
            }
            b.hits += 1;
            if b.hit.is_some_and(|h| !h.matches(b.hits)) {
                continue;
            }
            if let Some(msg) = &b.log {
                if let Ok(o) = OUTPUT.lock()
                    && let Some(out) = o.as_ref()
                {
                    out(&debug_eval::interpolate(msg, sc));
                }
                continue;
            }
            at_breakpoint = true;
        }
    }
    let Some(reason) = step.or(at_breakpoint.then_some(Reason::Breakpoint)) else {
        return;
    };
    let scope = scope.unwrap_or_else(|| scope_of(&desc, frame));
    let locals = scope
        .locals
        .iter()
        .map(|(name, raw, ty)| {
            let (ty, value) = debug_eval::describe(&debug_eval::from_raw(*raw, ty));
            Local {
                name: name.clone(),
                ty,
                value,
            }
        })
        .collect();
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
    s.scope = Some(Arc::new(scope));
    s.handles.clear();
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
        // Wake now and then so a host can keep its window alive while the game is parked.
        let (guard, timeout) = CHANGED
            .wait_timeout(s, Duration::from_millis(30))
            .unwrap_or_else(|e| e.into_inner());
        s = guard;
        if timeout.timed_out()
            && !s.go
            && let Ok(i) = IDLE.lock()
            && let Some(idle) = i.as_ref()
        {
            // Not under the state lock: the idle function may take a while.
            drop(s);
            idle();
            s = lock();
        }
    }
    s.go = false;
}
