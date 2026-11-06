// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The host's side of the TypeScript debugger (`--debug-port <port>`; design in
//! `docs/research/platypus-ts-debug-info.md`).
//!
//! The game is compiled as a debug build (`web_ui_ts::debug`: a check before every statement)
//! and a controller listens on `127.0.0.1:<port>`. The protocol is lines of text, so a person
//! with `telnet` or the DAP adapter of a later step can drive it:
//!
//! ```text
//! break src/main.ts:20   set a breakpoint (adds to the list; the list survives hot reloads);
//!                        `break src/main.ts:20 if i == 3` stops only when the condition holds
//! eval <expression>      the value of an expression over the stopped function's locals
//! catch on | off         stop at every `throw` statement
//! clear                  remove every breakpoint
//! pause                  stop at the next statement
//! continue | in | over | out
//! where                  the line the game is stopped at
//! locals                 the named locals of the function it is stopped in
//! stack                  the stop and its callers
//! detach                 clear everything and let the game go
//! ```
//!
//! Each command is answered with one line (`ok ...` or `error ...`), and a stop is announced
//! with `stopped <file>:<line> <breakpoint|pause|step>`. The game runs on the window's thread,
//! so a stopped game does not repaint or answer the window until it is resumed.
//!
//! Lines in files are mapped to the bundled source through `game.pdebug` (`platypus-build build`
//! writes it): after every compile the breakpoints are resolved again, which is how they
//! survive a hot reload that moves lines.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_ui_ts::debug::{self, Reason, Resume};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// The breakpoints as the debugger gave them: (file, line of that file).
static BREAKPOINTS: Mutex<Vec<FileBreakpoint>> = Mutex::new(Vec::new());

/// A breakpoint as the debugger gave it: a line of a file, and what makes it stop.
#[derive(Clone, Debug, Default)]
pub struct FileBreakpoint {
    pub file: String,
    pub line: u32,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
}

/// Tests share the process-wide debugger state: one at a time.
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

static BREAK_AT_START: AtomicBool = AtomicBool::new(false);
static INSPECT_BRK: AtomicBool = AtomicBool::new(false);

/// Was the run started with `--inspect-brk`?
pub fn inspect_brk() -> bool {
    INSPECT_BRK.load(Ordering::Relaxed)
}

/// `--inspect-brk`: the game stops before the program's first statement (its top level, then `onAwake` and the first update follow by stepping or `continue`).
pub fn break_at_start() {
    BREAK_AT_START.store(true, Ordering::Relaxed);
    INSPECT_BRK.store(true, Ordering::Relaxed);
}

/// True once, when `--inspect-brk` asked for the stop and the program is about to start.
pub fn take_first_statement_break() -> bool {
    BREAK_AT_START.swap(false, Ordering::Relaxed)
}

/// Is this run a debug build?
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Makes every compile a debug build.
pub fn turn_on() {
    ENABLED.store(true, Ordering::Relaxed);
    // A stopped game is the window's thread: keep answering the system so the window does not
    // go "not responding" while the debugger looks at it.
    #[cfg(windows)]
    debug::set_idle(Some(Box::new(pump_window_messages)));
}

/// Dispatches this thread's pending window messages (move, resize, close, activate). Events the
/// window raises are buffered by the event loop until the game goes on.
#[cfg(windows)]
fn pump_window_messages() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    // SAFETY: plain Win32 message calls on this thread with a zeroed message record.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        // A bounded number per call: a flood must not keep the game parked past a resume.
        for _ in 0..64 {
            if PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Starts the text controller on `port` (0 picks one) and makes every compile a debug build.
/// Returns the port.
pub fn enable(port: u16) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    turn_on();
    tracing::info!("debugger: listening on 127.0.0.1:{port}");
    std::thread::Builder::new()
        .name("debugger".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = std::thread::Builder::new()
                    .name("debugger client".into())
                    .spawn(move || serve(stream));
            }
        })?;
    Ok(port)
}

/// A bare `.ts` run directly has no `game.pdebug`: its one file is the whole bundle, so lines are
/// the bundle's. Holds that file's name.
static SINGLE_FILE: Mutex<Option<String>> = Mutex::new(None);

/// Notes that the game is one source file with this name (no sidecar needed).
pub fn note_single_file(name: &str) {
    if let Ok(mut s) = SINGLE_FILE.lock() {
        *s = Some(name.to_string());
    }
}

fn single_file() -> Option<String> {
    SINGLE_FILE.lock().ok()?.clone()
}

/// The bundled line of a file's line: through the sidecar, or as is for a one-file game.
fn bundle_line_of(file: &str, line: u32) -> Option<u32> {
    match super::crash::load_debug_info() {
        Some(i) => i.bundle_line(file, line),
        None => single_file().filter(|f| f == file).map(|_| line),
    }
}

/// Replaces the breakpoints of one file (a debugger sends each file's whole list). Returns,
/// per breakpoint, whether it was placed or why not.
pub fn set_file_breakpoints(file: &str, bps: &[FileBreakpoint]) -> Vec<Result<(), String>> {
    if let Ok(mut b) = BREAKPOINTS.lock() {
        b.retain(|x| x.file != file);
        b.extend(bps.iter().map(|x| FileBreakpoint {
            file: file.to_string(),
            ..x.clone()
        }));
    }
    let results = resolve_breakpoints();
    // The results follow the stored list: this file's breakpoints are the ones just added.
    let all = BREAKPOINTS.lock().map(|b| b.clone()).unwrap_or_default();
    let mut mine: Vec<Result<(), String>> = all
        .iter()
        .zip(results)
        .filter(|(x, _)| x.file == file)
        .map(|(_, r)| r)
        .collect();
    // Not a debug build (nothing resolved): none of them is placed.
    mine.resize_with(bps.len(), || {
        Err("the game is not running as a debug build".into())
    });
    mine
}

/// The file and line of a line of the bundled source.
pub fn locate(line: u32) -> Option<(String, u32)> {
    match super::crash::load_debug_info() {
        Some(i) => i.locate(line).map(|(f, l)| (f.to_string(), l as u32)),
        None => single_file().map(|f| (f, line)),
    }
}

/// The program's files, as the sidecar names them.
pub fn files() -> Vec<String> {
    match super::crash::load_debug_info() {
        Some(i) => i.files().map(str::to_string).collect(),
        None => single_file().into_iter().collect(),
    }
}

/// Set when a debugger has said it is done configuring (breakpoints sent): the game may start.
static CONFIGURED: (Mutex<bool>, std::sync::Condvar) =
    (Mutex::new(false), std::sync::Condvar::new());

/// Lets a game that waits (`wait_for_configuration`) go on.
pub fn configured() {
    if let Ok(mut c) = CONFIGURED.0.lock() {
        *c = true;
        CONFIGURED.1.notify_all();
    }
}

/// Blocks until a debugger is configured, so the game does not run past a breakpoint that is
/// still on its way (the `--inspect-brk` idea).
pub fn wait_for_configuration() {
    let Ok(mut c) = CONFIGURED.0.lock() else {
        return;
    };
    while !*c {
        c = CONFIGURED.1.wait(c).unwrap_or_else(|e| e.into_inner());
    }
}

/// Resolves the breakpoints against the code just compiled (the bundled lines moved if the
/// source did): called after every compile. Returns, per stored breakpoint, whether it was placed.
pub fn resolve_breakpoints() -> Vec<Result<(), String>> {
    if !enabled() {
        return Vec::new();
    }
    let wanted = BREAKPOINTS.lock().map(|b| b.clone()).unwrap_or_default();
    // Only the ones with a line in the bundle go to the debugger; the results map back.
    let mut specs = Vec::new();
    let mut at = Vec::new();
    for (i, w) in wanted.iter().enumerate() {
        if let Some(line) = bundle_line_of(&w.file, w.line) {
            specs.push(debug::BreakpointSpec {
                line,
                condition: w.condition.clone(),
                hit_condition: w.hit_condition.clone(),
                log_message: w.log_message.clone(),
            });
            at.push(i);
        }
    }
    let placed = debug::set_breakpoint_specs(&specs);
    let mut results: Vec<Result<(), String>> = wanted
        .iter()
        .map(|_| Err("no such file (game.pdebug does not list it)".to_string()))
        .collect();
    for (i, r) in at.into_iter().zip(placed) {
        results[i] = r;
    }
    let missing = results.iter().filter(|r| r.is_err()).count();
    if missing > 0 {
        tracing::warn!(
            "debugger: {missing} of {} breakpoints could not be set (no game.pdebug, an unknown file, or a bad condition)",
            wanted.len()
        );
    }
    results
}

/// `file:line` of a line of the bundled source, for messages.
fn describe(line: u32) -> String {
    locate(line)
        .map(|(f, l)| format!("{f}:{l}"))
        .unwrap_or_else(|| format!("line {line}"))
}

fn serve(stream: TcpStream) {
    let Ok(read) = stream.try_clone() else {
        return;
    };
    let out = Arc::new(Mutex::new(stream));
    let say = {
        let out = out.clone();
        move |text: &str| {
            if let Ok(mut s) = out.lock() {
                let _ = writeln!(s, "{text}");
            }
        }
    };
    // Announces each stop once, whoever resumed the game.
    let watching = Arc::new(AtomicBool::new(true));
    {
        let (say, watching) = (say.clone(), watching.clone());
        let _ = std::thread::Builder::new()
            .name("debugger events".into())
            .spawn(move || {
                let mut last = None;
                while watching.load(Ordering::Relaxed) {
                    match debug::wait_stopped(Duration::from_millis(100)) {
                        Some(stop) if last != Some((stop.line, stop.sp)) => {
                            last = Some((stop.line, stop.sp));
                            let why = match stop.reason {
                                Reason::Breakpoint => "breakpoint",
                                Reason::Pause => "pause",
                                Reason::Step => "step",
                                Reason::Exception => "exception",
                            };
                            say(&format!("stopped {} {why}", describe(stop.line)));
                        }
                        Some(_) => std::thread::sleep(Duration::from_millis(50)),
                        None => last = None,
                    }
                }
            });
    }
    for line in BufReader::new(read).lines().map_while(Result::ok) {
        say(&command(line.trim()));
    }
    watching.store(false, Ordering::Relaxed);
    // A terminal that closes must not leave the game stopped.
    debug::detach();
}

/// Runs one command and returns the reply line.
fn command(line: &str) -> String {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    match word {
        "break" => {
            let (place, condition) = match rest.split_once(" if ") {
                Some((p, c)) => (p.trim(), Some(c.trim().to_string())),
                None => (rest.trim(), None),
            };
            let Some((file, n)) = place.rsplit_once(':') else {
                return "error: break <file>:<line> [if <condition>]".into();
            };
            let Ok(n) = n.parse::<u32>() else {
                return "error: the line is not a number".into();
            };
            if let Ok(mut b) = BREAKPOINTS.lock() {
                b.push(FileBreakpoint {
                    file: file.to_string(),
                    line: n,
                    condition,
                    ..FileBreakpoint::default()
                });
            }
            let results = resolve_breakpoints();
            match results.last() {
                Some(Err(e)) => format!("error: {e}"),
                _ => format!("ok break {file}:{n}"),
            }
        }
        "clear" => {
            if let Ok(mut b) = BREAKPOINTS.lock() {
                b.clear();
            }
            debug::set_breakpoints(&[]);
            "ok".into()
        }
        "pause" => {
            debug::pause();
            "ok".into()
        }
        "continue" => resume(Resume::Continue),
        "in" => resume(Resume::StepIn),
        "over" => resume(Resume::StepOver),
        "out" => resume(Resume::StepOut),
        "where" => match debug::stopped() {
            Some(s) => format!("ok stopped {}", describe(s.line)),
            None => "ok running".into(),
        },
        "catch" => match rest.trim() {
            "on" | "off" => {
                debug::set_break_on_throw(rest.trim() == "on");
                "ok".into()
            }
            _ => "error: catch on|off".into(),
        },
        "eval" => match debug::evaluate(rest.trim()) {
            Ok(v) => format!("ok {}: {} = {}", rest.trim(), v.ty, v.value),
            Err(e) => format!("error: {e}"),
        },
        "locals" => match debug::stopped() {
            Some(s) => format!(
                "ok {}",
                s.locals
                    .iter()
                    .map(|l| format!("{}:{}={}", l.name, l.ty, l.value))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            None => "error: the game is not stopped".into(),
        },
        "stack" => match debug::stopped() {
            Some(s) => format!(
                "ok {}",
                s.frames
                    .iter()
                    .map(|f| format!("{} {}", f.name, describe(f.line)))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            None => "error: the game is not stopped".into(),
        },
        "detach" => {
            if let Ok(mut b) = BREAKPOINTS.lock() {
                b.clear();
            }
            debug::detach();
            "ok".into()
        }
        "" => "error: empty command".into(),
        other => format!("error: unknown command {other}"),
    }
}

fn resume(how: Resume) -> String {
    if debug::stopped().is_none() {
        return "error: the game is not stopped".into();
    }
    debug::resume(how);
    "ok".into()
}

#[cfg(test)]
mod tests {
    use super::command;

    // M9-TS-3397
    #[test]
    fn bad_commands_are_answered_with_an_error_line() {
        let _serial = super::test_lock();
        assert!(command("").starts_with("error"));
        assert!(command("bogus").starts_with("error: unknown command"));
        assert!(command("break nolinenumber").starts_with("error"));
        assert!(command("break src/a.ts:x").starts_with("error"));
        assert!(command("continue").starts_with("error: the game is not stopped"));
        assert_eq!(command("where"), "ok running");
        assert!(command("locals").starts_with("error"));
        assert!(command("stack").starts_with("error"));
    }
}
