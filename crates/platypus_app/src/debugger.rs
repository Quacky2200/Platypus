// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The host's side of the TypeScript debugger (`--debug-port <port>`; design in
//! `docs/research/platypus-ts-debug-info.md`).
//!
//! The game is compiled as a debug build (`platypus_ts::debug`: a check before every statement)
//! and a controller listens on `127.0.0.1:<port>`. The protocol is lines of text, so a person
//! with `telnet` or the DAP adapter of a later step can drive it:
//!
//! ```text
//! break src/main.ts:20   set a breakpoint (adds to the list; the list survives hot reloads)
//! clear                  remove every breakpoint
//! pause                  stop at the next statement
//! continue | in | over | out
//! where                  the line the game is stopped at
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

use platypus_ts::debug::{self, Reason, Resume};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// The breakpoints as the debugger gave them: (file, line of that file).
static BREAKPOINTS: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());

/// Is this run a debug build?
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Starts the controller on `port` (0 picks one) and makes every compile a debug build.
/// Returns the port.
pub fn enable(port: u16) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    ENABLED.store(true, Ordering::Relaxed);
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

/// Resolves the breakpoints against the code just compiled (the bundled lines moved if the
/// source did): called after every compile.
pub fn resolve_breakpoints() {
    if !enabled() {
        return;
    }
    let wanted = BREAKPOINTS.lock().map(|b| b.clone()).unwrap_or_default();
    let info = super::crash::load_debug_info();
    let lines: Vec<u32> = wanted
        .iter()
        .filter_map(|(file, line)| match &info {
            Some(i) => i.bundle_line(file, *line),
            None => None,
        })
        .collect();
    if lines.len() < wanted.len() {
        tracing::warn!(
            "debugger: {} of {} breakpoints could not be placed (no game.pdebug, or no such file)",
            wanted.len() - lines.len(),
            wanted.len()
        );
    }
    debug::set_breakpoints(&lines);
}

/// `file:line` of a line of the bundled source, for messages.
fn describe(line: u32) -> String {
    super::crash::load_debug_info()
        .and_then(|i| i.locate(line).map(|(f, l)| format!("{f}:{l}")))
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
}

/// Runs one command and returns the reply line.
fn command(line: &str) -> String {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    match word {
        "break" => {
            let Some((file, n)) = rest.trim().rsplit_once(':') else {
                return "error: break <file>:<line>".into();
            };
            let Ok(n) = n.parse::<u32>() else {
                return "error: the line is not a number".into();
            };
            if let Ok(mut b) = BREAKPOINTS.lock() {
                b.push((file.to_string(), n));
            }
            resolve_breakpoints();
            format!("ok break {file}:{n}")
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
        assert!(command("").starts_with("error"));
        assert!(command("bogus").starts_with("error: unknown command"));
        assert!(command("break nolinenumber").starts_with("error"));
        assert!(command("break src/a.ts:x").starts_with("error"));
        assert!(command("continue").starts_with("error: the game is not stopped"));
        assert_eq!(command("where"), "ok running");
    }
}
