// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A Debug Adapter Protocol server for `platypus-app --dap-port <port>`: editors that speak DAP
//! (VS Code, the editor of a later step) attach to the running game and set breakpoints, pause,
//! continue and step. It drives the same debugger as the text controller (`debugger.rs`,
//! `web_ui_ts::debug`); the guide is `docs/debugger.md`.
//!
//! The game does not start until the debugger sends `configurationDone`, so breakpoints set
//! while the editor connects are in place before the first statement runs.
//!
//! Supported: `initialize`, `attach`/`launch`, `setBreakpoints`, `setExceptionBreakpoints`,
//! `configurationDone`, `threads`, `stackTrace` (the stop and the callers found on the stack),
//! `scopes`/`variables` (the innermost frame's locals, objects and arrays opened), `evaluate` (watches, hovers, the console), `continue`, `next`, `stepIn`,
//! `stepOut`, `pause`, `disconnect`. One thread, id 1, is the game's.

use crate::debugger;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_ui_ts::debug::{self, Reason, Resume};

/// Starts the server on `port` (0 picks one) and makes every compile a debug build. Returns the
/// port. One debugger at a time is served; the next connection is served after it leaves.
pub fn enable(port: u16) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    debugger::turn_on();
    tracing::info!("debugger: DAP server listening on 127.0.0.1:{port}");
    std::thread::Builder::new()
        .name("dap".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let Ok(read) = stream.try_clone() else {
                    continue;
                };
                serve(std::io::BufReader::new(read), Arc::new(Mutex::new(stream)));
            }
        })?;
    Ok(port)
}

/// Reads one framed message (`Content-Length: n`, a blank line, n bytes of JSON).
fn read_message<R: BufRead>(r: &mut R) -> Option<Value> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length:") {
            len = n.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0u8; len?];
    r.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

struct Sender<W: Write> {
    out: Arc<Mutex<W>>,
    seq: AtomicI64,
}

impl<W: Write> Sender<W> {
    fn send(&self, mut msg: Value) {
        msg["seq"] = json!(self.seq.fetch_add(1, Ordering::SeqCst) + 1);
        let body = msg.to_string();
        if let Ok(mut w) = self.out.lock() {
            let _ = write!(w, "Content-Length: {}\r\n\r\n{body}", body.len());
            let _ = w.flush();
        }
    }

    fn event(&self, event: &str, body: Value) {
        self.send(json!({"type": "event", "event": event, "body": body}));
    }

    fn respond(&self, req: &Value, success: bool, body: Value, message: Option<&str>) {
        let mut r = json!({
            "type": "response",
            "request_seq": req["seq"],
            "success": success,
            "command": req["command"],
            "body": body,
        });
        if let Some(m) = message {
            r["message"] = json!(m);
        }
        self.send(r);
    }
}

/// The file (as `game.pdebug` names it) an editor's path is: the known file the path ends with.
pub(crate) fn file_of(path: &str) -> String {
    let norm = path.replace('\\', "/");
    debugger::files()
        .into_iter()
        .filter(|f| norm.ends_with(f.as_str()))
        .max_by_key(String::len)
        .unwrap_or(norm)
}

/// The path an editor opens for a file of the program.
pub(crate) fn path_of(file: &str) -> String {
    match crate::crash::game_dir() {
        Some(d) => d.join(file).to_string_lossy().into_owned(),
        None => file.to_string(),
    }
}

/// Serves one debugger until it disconnects.
pub fn serve<R: BufRead, W: Write + Send + 'static>(mut reader: R, out: Arc<Mutex<W>>) {
    let tx = Arc::new(Sender {
        out,
        seq: AtomicI64::new(0),
    });
    let watching = Arc::new(AtomicBool::new(true));
    // Log points write to the debugger's console.
    {
        let tx = tx.clone();
        debug::set_output(Some(Box::new(move |m| {
            tx.event(
                "output",
                json!({"category": "console", "output": format!("{m}\n")}),
            );
        })));
    }
    {
        let (tx, watching) = (tx.clone(), watching.clone());
        let _ = std::thread::Builder::new()
            .name("dap events".into())
            .spawn(move || {
                let mut last = None;
                while watching.load(Ordering::Relaxed) {
                    match debug::wait_stopped(Duration::from_millis(100)) {
                        Some(s) if last != Some((s.line, s.sp)) => {
                            last = Some((s.line, s.sp));
                            let reason = match s.reason {
                                Reason::Breakpoint => "breakpoint",
                                Reason::Pause => "pause",
                                Reason::Step => "step",
                                Reason::Exception => "exception",
                            };
                            tx.event(
                                "stopped",
                                json!({"reason": reason, "threadId": 1, "allThreadsStopped": true}),
                            );
                        }
                        Some(_) => std::thread::sleep(Duration::from_millis(50)),
                        None => last = None,
                    }
                }
            });
    }
    while let Some(req) = read_message(&mut reader) {
        if req["type"] != "request" {
            continue;
        }
        if !handle(&tx, &req) {
            break;
        }
    }
    watching.store(false, Ordering::Relaxed);
    debug::set_output(None);
    debug::detach();
    // A debugger that went away without configuring must not leave the game waiting.
    debugger::configured();
}

/// Handles one request; `false` ends the session.
fn handle<W: Write>(tx: &Sender<W>, req: &Value) -> bool {
    let args = &req["arguments"];
    match req["command"].as_str().unwrap_or_default() {
        "initialize" => {
            tx.respond(
                req,
                true,
                json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsConditionalBreakpoints": true,
                    "supportsHitConditionalBreakpoints": true,
                    "supportsLogPoints": true,
                    "supportsEvaluateForHovers": true,
                    "supportsTerminateRequest": false,
                    "exceptionBreakpointFilters": [
                        {"filter": "all", "label": "All throws", "default": false}
                    ],
                }),
                None,
            );
            tx.event("initialized", json!({}));
        }
        "attach" | "launch" => tx.respond(req, true, json!({}), None),
        "setBreakpoints" => {
            let path = args["source"]["path"].as_str().unwrap_or_default();
            let file = file_of(path);
            let text = |b: &Value, k: &str| b[k].as_str().map(str::to_string);
            let wanted: Vec<debugger::FileBreakpoint> = args["breakpoints"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|b| {
                            Some(debugger::FileBreakpoint {
                                file: file.clone(),
                                line: b["line"].as_u64()? as u32,
                                condition: text(b, "condition"),
                                hit_condition: text(b, "hitCondition"),
                                log_message: text(b, "logMessage"),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let placed = debugger::set_file_breakpoints(&file, &wanted);
            let bps: Vec<Value> = wanted
                .iter()
                .zip(placed)
                .map(|(b, r)| match r {
                    Ok(()) => json!({"verified": true, "line": b.line}),
                    Err(e) => json!({"verified": false, "line": b.line, "message": e}),
                })
                .collect();
            tx.respond(req, true, json!({"breakpoints": bps}), None);
        }
        "setExceptionBreakpoints" => {
            let on = args["filters"]
                .as_array()
                .is_some_and(|f| f.iter().any(|x| x == "all"));
            debug::set_break_on_throw(on);
            tx.respond(req, true, json!({}), None);
        }
        "configurationDone" => {
            debugger::configured();
            tx.respond(req, true, json!({}), None);
        }
        "threads" => tx.respond(
            req,
            true,
            json!({"threads": [{"id": 1, "name": "game"}]}),
            None,
        ),
        "stackTrace" => {
            let frames: Vec<Value> = debug::stopped()
                .map(|s| {
                    s.frames
                        .iter()
                        .enumerate()
                        .map(|(i, f)| {
                            let at = debugger::locate(f.line);
                            let name = if f.name.is_empty() { "game" } else { &f.name };
                            let mut v = json!({"id": i, "name": name, "line": at.as_ref().map_or(f.line, |a| a.1), "column": 1});
                            if let Some((file, _)) = &at {
                                v["source"] = json!({"name": file, "path": path_of(file)});
                            }
                            v
                        })
                        .collect()
                })
                .unwrap_or_default();
            let n = frames.len();
            tx.respond(
                req,
                true,
                json!({"stackFrames": frames, "totalFrames": n}),
                None,
            );
        }
        // Only the innermost frame keeps its locals (callers' registers were not saved).
        "scopes" => {
            let top = args["frameId"].as_i64().unwrap_or(0) == 0;
            let scopes = if top && debug::stopped().is_some() {
                vec![json!({"name": "Locals", "variablesReference": 1, "expensive": false})]
            } else {
                Vec::new()
            };
            tx.respond(req, true, json!({"scopes": scopes}), None);
        }
        "variables" => {
            let reference = args["variablesReference"].as_u64().unwrap_or(0) as u32;
            let vars: Vec<Value> = debug::variables(reference)
                .iter()
                .map(variable_json)
                .collect();
            tx.respond(req, true, json!({"variables": vars}), None);
        }
        // A watch, a hover or the debug console: an expression over the innermost frame's locals.
        "evaluate" => {
            let expr = args["expression"].as_str().unwrap_or_default();
            if args["frameId"].as_i64().unwrap_or(0) != 0 {
                tx.respond(
                    req,
                    false,
                    json!({}),
                    Some("only the innermost frame's locals are available"),
                );
            } else {
                match debug::evaluate(expr) {
                    Ok(v) => tx.respond(
                        req,
                        true,
                        json!({"result": v.value, "type": v.ty, "variablesReference": v.handle}),
                        None,
                    ),
                    Err(e) => tx.respond(req, false, json!({}), Some(&e)),
                }
            }
        }
        "continue" => {
            debug::resume(Resume::Continue);
            tx.respond(req, true, json!({"allThreadsContinued": true}), None);
        }
        "next" => step(tx, req, Resume::StepOver),
        "stepIn" => step(tx, req, Resume::StepIn),
        "stepOut" => step(tx, req, Resume::StepOut),
        "pause" => {
            debug::pause();
            tx.respond(req, true, json!({}), None);
        }
        "disconnect" => {
            debug::detach();
            tx.respond(req, true, json!({}), None);
            return false;
        }
        other => tx.respond(
            req,
            false,
            json!({}),
            Some(&format!("{other} is not supported")),
        ),
    }
    true
}

fn variable_json(v: &debug::Variable) -> Value {
    json!({"name": v.name, "value": v.value, "type": v.ty, "variablesReference": v.handle})
}

fn step<W: Write>(tx: &Sender<W>, req: &Value, how: Resume) {
    debug::resume(how);
    tx.respond(req, true, json!({}), None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Frames requests, runs a session over them and returns every message the server sent.
    fn session(requests: &[Value]) -> Vec<Value> {
        let mut input = Vec::new();
        for r in requests {
            let body = r.to_string();
            input.extend(format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes());
        }
        let out = Arc::new(Mutex::new(Vec::<u8>::new()));
        serve(Cursor::new(input), out.clone());
        let bytes = out.lock().unwrap().clone();
        let mut cur = Cursor::new(bytes);
        let mut msgs = Vec::new();
        while let Some(m) = read_message(&mut cur) {
            msgs.push(m);
        }
        msgs
    }

    // M9-TS-3398
    #[test]
    fn a_session_initialises_answers_every_request_and_ends_on_disconnect() {
        let _serial = crate::debugger::test_lock();
        let msgs = session(&[
            json!({"seq": 1, "type": "request", "command": "initialize", "arguments": {}}),
            json!({"seq": 2, "type": "request", "command": "attach", "arguments": {}}),
            json!({"seq": 3, "type": "request", "command": "setBreakpoints",
                   "arguments": {"source": {"path": "C:/g/src/none.ts"}, "breakpoints": [{"line": 4}]}}),
            json!({"seq": 4, "type": "request", "command": "configurationDone"}),
            json!({"seq": 5, "type": "request", "command": "threads"}),
            json!({"seq": 6, "type": "request", "command": "stackTrace", "arguments": {"threadId": 1}}),
            json!({"seq": 7, "type": "request", "command": "setVariable", "arguments": {}}),
            json!({"seq": 8, "type": "request", "command": "disconnect"}),
            json!({"seq": 9, "type": "request", "command": "threads"}),
        ]);
        let responses: Vec<&Value> = msgs.iter().filter(|m| m["type"] == "response").collect();
        // Everything up to the disconnect is answered; nothing after it.
        assert_eq!(responses.len(), 8, "{msgs:?}");
        assert!(msgs.iter().any(|m| m["event"] == "initialized"));
        assert_eq!(
            responses[0]["body"]["supportsConfigurationDoneRequest"],
            true
        );
        // The file is unknown (no game.pdebug in the test): not placed, but answered.
        assert_eq!(responses[2]["body"]["breakpoints"][0]["verified"], false);
        assert_eq!(responses[2]["body"]["breakpoints"][0]["line"], 4);
        assert_eq!(responses[4]["body"]["threads"][0]["id"], 1);
        assert_eq!(
            responses[5]["body"]["stackFrames"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(responses[6]["success"], false);
        // Replies carry the request's number and the server's own sequence.
        assert_eq!(responses[1]["request_seq"], 2);
        assert!(msgs.iter().all(|m| m["seq"].as_i64().unwrap_or(0) > 0));
    }
}
