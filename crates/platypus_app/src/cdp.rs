// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A Chrome DevTools Protocol server for `platypus-app --inspect-brk` (the default tool): what
//! `node --inspect-brk` offers, so Chrome DevTools (`chrome://inspect`, "Discover network
//! targets"), VS Code's JavaScript debugger and Node's tooling find the game on `127.0.0.1:9229`,
//! list it at `/json`, and attach over a WebSocket. It drives the same debugger as the text
//! controller and the DAP server (`debugger.rs`, `web_ui_ts::debug`); the guide is
//! `docs/debugger.md`.
//!
//! Like Node's `--inspect-brk`, the game does not start until the debugger sends
//! `Runtime.runIfWaitingForDebugger`, and it stops at the first statement ("Break on start").
//!
//! Supported: `/json/version`, `/json`, `/json/list`; `Runtime.enable`/`evaluate`/`getProperties`/
//! `runIfWaitingForDebugger`; `Debugger.enable`, `setBreakpointByUrl` (with conditions),
//! `removeBreakpoint`, `setPauseOnExceptions`, `pause`, `resume`, `stepOver`/`stepInto`/`stepOut`,
//! `evaluateOnCallFrame`, `getScriptSource`. Locals of the innermost frame are the one scope.

use crate::dap::{file_of, path_of};
use crate::debugger::{self, FileBreakpoint};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_ui_ts::debug::{self, Reason, Resume, Stop, Variable};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Starts the server on `port` (0 picks one) and makes every compile a debug build. Returns the
/// port. Each connection (an HTTP query or a debugger's WebSocket) gets its own thread.
pub fn enable(port: u16) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    debugger::turn_on();
    std::thread::Builder::new()
        .name("cdp".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = std::thread::Builder::new()
                    .name("cdp connection".into())
                    .spawn(move || connection(stream, port));
            }
        })?;
    Ok(port)
}

// ---- WebSocket and HTTP plumbing ----

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, c) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// The `Sec-WebSocket-Accept` answer to a client's key.
fn accept_key(key: &str) -> String {
    base64(&sha1(format!("{key}{GUID}").as_bytes()))
}

/// Reads one text message (continuation frames joined); answers pings; `None` when the peer
/// closes or the connection fails.
fn read_text<R: Read>(r: &mut R, tx: &Tx) -> Option<String> {
    let mut text: Vec<u8> = Vec::new();
    loop {
        let mut head = [0u8; 2];
        r.read_exact(&mut head).ok()?;
        let (fin, op) = (head[0] & 0x80 != 0, head[0] & 0x0f);
        let masked = head[1] & 0x80 != 0;
        let mut len = (head[1] & 0x7f) as u64;
        if len == 126 {
            let mut b = [0u8; 2];
            r.read_exact(&mut b).ok()?;
            len = u16::from_be_bytes(b) as u64;
        } else if len == 127 {
            let mut b = [0u8; 8];
            r.read_exact(&mut b).ok()?;
            len = u64::from_be_bytes(b);
        }
        let mut mask = [0u8; 4];
        if masked {
            r.read_exact(&mut mask).ok()?;
        }
        if len > 64 << 20 {
            return None;
        }
        let mut payload = vec![0u8; len as usize];
        r.read_exact(&mut payload).ok()?;
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        match op {
            8 => return None,
            9 => tx.frame(10, &payload),
            1 | 0 => {
                text.extend_from_slice(&payload);
                if fin {
                    return String::from_utf8(text).ok();
                }
            }
            _ => {}
        }
    }
}

struct Tx {
    out: Mutex<TcpStream>,
}

impl Tx {
    fn frame(&self, op: u8, payload: &[u8]) {
        let mut f = vec![0x80 | op];
        match payload.len() {
            n if n < 126 => f.push(n as u8),
            n if n < 65536 => {
                f.push(126);
                f.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                f.push(127);
                f.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        f.extend_from_slice(payload);
        if let Ok(mut o) = self.out.lock() {
            let _ = o.write_all(&f);
        }
    }

    fn send(&self, v: &Value) {
        self.frame(1, v.to_string().as_bytes());
    }

    fn event(&self, method: &str, params: Value) {
        self.send(&json!({"method": method, "params": params}));
    }

    fn respond(&self, id: &Value, result: Value) {
        self.send(&json!({"id": id, "result": result}));
    }

    fn fail(&self, id: &Value, message: &str) {
        self.send(&json!({"id": id, "error": {"code": -32601, "message": message}}));
    }
}

fn connection(stream: TcpStream, port: u16) {
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 {
            return;
        }
        let h = h.trim();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let host = headers
        .get("host")
        .cloned()
        .unwrap_or_else(|| format!("127.0.0.1:{port}"));
    let mut out = stream;
    if let Some(key) = headers.get("sec-websocket-key") {
        let reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(key)
        );
        if out.write_all(reply.as_bytes()).is_err() {
            return;
        }
        serve(reader, out);
        return;
    }
    let body = match path.trim_end_matches('/') {
        "/json/version" => json!({"Browser": "platypus-ts", "Protocol-Version": "1.3"}),
        "/json" | "/json/list" => json!([target(&host)]),
        _ => {
            let _ = out.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
    };
    let body = body.to_string();
    let _ = write!(
        out,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// The one debuggable target: the game.
fn target(host: &str) -> Value {
    let name = crate::crash::game_dir()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "game".into());
    let first = debugger::files().into_iter().next().unwrap_or_default();
    json!({
        "description": "platypus-ts game",
        "devtoolsFrontendUrl": format!("devtools://devtools/bundled/js_app.html?experiments=true&v8only=true&ws={host}/game"),
        "id": "game",
        "title": name,
        "type": "node",
        "url": url_of(&first),
        "webSocketDebuggerUrl": format!("ws://{host}/game"),
    })
}

// ---- The session ----

struct Session {
    /// Breakpoints by file: id, definition.
    bps: Mutex<HashMap<String, Vec<(String, FileBreakpoint)>>>,
    next: AtomicU64,
    /// The first stop is "Break on start".
    first: AtomicBool,
}

fn url_of(file: &str) -> String {
    // DevTools wants absolute paths; the game folder may have been given relative.
    let raw = path_of(file);
    let abs = std::path::absolute(&raw).unwrap_or_else(|_| raw.into());
    let p = abs.to_string_lossy().replace('\\', "/");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}")
    }
}

fn script_id(file: &str) -> String {
    debugger::files()
        .iter()
        .position(|f| f == file)
        .unwrap_or(0)
        .to_string()
}

fn file_of_url(url: &str) -> String {
    let p = url
        .strip_prefix("file:///")
        .or_else(|| url.strip_prefix("file://"))
        .unwrap_or(url);
    file_of(p)
}

/// A debugger value as a CDP `RemoteObject`.
fn remote(v: &Variable) -> Value {
    match v.ty.as_str() {
        "number" => match v.value.parse::<f64>() {
            Ok(n) if n.is_finite() => json!({"type": "number", "value": n, "description": v.value}),
            _ => json!({"type": "number", "unserializableValue": v.value, "description": v.value}),
        },
        "boolean" => json!({"type": "boolean", "value": v.value == "true"}),
        "string" => json!({"type": "string", "value": v.value.trim_matches('"')}),
        "null" => {
            json!({"type": "object", "subtype": "null", "value": null, "description": "null"})
        }
        "undefined" => json!({"type": "undefined"}),
        // Objects and arrays, even empty ones (no handle: nothing to open).
        ty => {
            let mut o = json!({"type": "object", "className": if ty == "array" { "Array" } else { ty }, "description": v.value});
            if ty == "array" {
                o["subtype"] = json!("array");
            }
            if v.handle != 0 {
                o["objectId"] = json!(format!("h:{}", v.handle));
            }
            o
        }
    }
}

fn properties(vars: &[Variable]) -> Value {
    let list: Vec<Value> = vars
        .iter()
        .map(|v| json!({"name": v.name, "value": remote(v), "configurable": true, "enumerable": true, "writable": false, "isOwn": true}))
        .collect();
    json!({"result": list})
}

fn paused_params(s: &Stop, session: &Session, first: bool) -> Value {
    let frames: Vec<Value> = s
        .frames
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let at = debugger::locate(f.line);
            let (sid, url, line) = match &at {
                Some((file, l)) => (script_id(file), url_of(file), *l),
                None => ("0".into(), String::new(), f.line),
            };
            let name = if f.name.is_empty() { "(game)" } else { &f.name };
            let scopes = if i == 0 {
                json!([{"type": "local", "name": "Locals", "object": {"type": "object", "className": "Object", "description": "Locals", "objectId": "scope"}}])
            } else {
                json!([])
            };
            json!({
                "callFrameId": i.to_string(),
                "functionName": name,
                "location": {"scriptId": sid, "lineNumber": line.saturating_sub(1), "columnNumber": 0},
                "url": url,
                "scopeChain": scopes,
                "this": {"type": "undefined"},
            })
        })
        .collect();
    let mut hit = Vec::new();
    if s.reason == Reason::Breakpoint
        && let Some((file, line)) = debugger::locate(s.line)
        && let Ok(bps) = session.bps.lock()
    {
        for (id, b) in bps.get(&file).into_iter().flatten() {
            if b.line == line {
                hit.push(id.clone());
            }
        }
    }
    let reason = match s.reason {
        _ if first => "Break on start",
        Reason::Exception => "exception",
        Reason::Pause => "debugCommand",
        _ => "other",
    };
    json!({"callFrames": frames, "reason": reason, "hitBreakpoints": hit})
}

fn serve(mut reader: BufReader<TcpStream>, out: TcpStream) {
    let tx = Arc::new(Tx {
        out: Mutex::new(out),
    });
    let session = Arc::new(Session {
        bps: Mutex::new(HashMap::new()),
        next: AtomicU64::new(1),
        first: AtomicBool::new(debugger::inspect_brk()),
    });
    let watching = Arc::new(AtomicBool::new(true));
    {
        let tx = tx.clone();
        debug::set_output(Some(Box::new(move |m| {
            tx.event(
                "Runtime.consoleAPICalled",
                json!({"type": "log", "args": [{"type": "string", "value": m}], "executionContextId": 1, "timestamp": 0}),
            );
        })));
    }
    {
        let (tx, session, watching) = (tx.clone(), session.clone(), watching.clone());
        let _ = std::thread::Builder::new()
            .name("cdp events".into())
            .spawn(move || {
                let mut last = None;
                while watching.load(Ordering::Relaxed) {
                    match debug::wait_stopped(Duration::from_millis(100)) {
                        Some(s) if last != Some((s.line, s.sp)) => {
                            last = Some((s.line, s.sp));
                            let first = session.first.swap(false, Ordering::Relaxed);
                            tx.event("Debugger.paused", paused_params(&s, &session, first));
                        }
                        Some(_) => std::thread::sleep(Duration::from_millis(50)),
                        None => last = None,
                    }
                }
            });
    }
    while let Some(text) = read_text(&mut reader, &tx) {
        let Ok(req) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        handle(&tx, &session, &req);
    }
    watching.store(false, Ordering::Relaxed);
    debug::set_output(None);
    debug::detach();
    debugger::configured();
}

fn eval_result(tx: &Tx, id: &Value, expr: &str, frame: i64) {
    if frame != 0 {
        tx.fail(id, "only the innermost frame's locals are available");
        return;
    }
    match debug::evaluate(expr) {
        Ok(v) => tx.respond(id, json!({"result": remote(&v)})),
        Err(e) => tx.respond(
            id,
            json!({
                "result": {"type": "object", "subtype": "error", "className": "Error", "description": e},
                "exceptionDetails": {"exceptionId": 1, "text": e, "lineNumber": 0, "columnNumber": 0},
            }),
        ),
    }
}

fn apply(session: &Session, file: &str) {
    let list: Vec<FileBreakpoint> = session
        .bps
        .lock()
        .map(|b| {
            b.get(file)
                .map(|v| v.iter().map(|(_, b)| b.clone()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    debugger::set_file_breakpoints(file, &list);
}

fn handle(tx: &Arc<Tx>, session: &Session, req: &Value) {
    let id = &req["id"];
    let p = &req["params"];
    match req["method"].as_str().unwrap_or_default() {
        "Runtime.enable" => {
            tx.respond(id, json!({}));
            tx.event(
                "Runtime.executionContextCreated",
                json!({"context": {"id": 1, "origin": "", "name": "platypus-ts", "uniqueId": "1"}}),
            );
        }
        "Debugger.enable" => {
            tx.respond(id, json!({"debuggerId": "platypus-1"}));
            for (i, file) in debugger::files().iter().enumerate() {
                let src = std::fs::read_to_string(path_of(file)).unwrap_or_default();
                tx.event(
                    "Debugger.scriptParsed",
                    json!({"scriptId": i.to_string(), "url": url_of(file), "startLine": 0, "startColumn": 0,
                           "endLine": src.lines().count(), "endColumn": 0, "executionContextId": 1,
                           "hash": "", "isLiveEdit": false, "sourceMapURL": "", "hasSourceURL": false,
                           "isModule": true, "length": src.len()}),
                );
            }
        }
        "Runtime.runIfWaitingForDebugger" => {
            tx.respond(id, json!({}));
            debugger::configured();
        }
        "Debugger.getScriptSource" => {
            let n: usize = p["scriptId"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let src = debugger::files()
                .get(n)
                .and_then(|f| std::fs::read_to_string(path_of(f)).ok())
                .unwrap_or_default();
            tx.respond(id, json!({"scriptSource": src}));
        }
        "Debugger.setBreakpointByUrl" => {
            let file = match (p["url"].as_str(), p["urlRegex"].as_str()) {
                (Some(u), _) => file_of_url(u),
                (None, Some(r)) => file_of(
                    &r.replace("\\", "")
                        .replace(".*", "")
                        .replace(['^', '$'], ""),
                ),
                _ => String::new(),
            };
            let line = p["lineNumber"].as_u64().unwrap_or(0) as u32 + 1;
            let bp_id = format!("{}:{}", session.next.fetch_add(1, Ordering::Relaxed), line);
            let bp = FileBreakpoint {
                file: file.clone(),
                line,
                condition: p["condition"]
                    .as_str()
                    .filter(|c| !c.is_empty())
                    .map(str::to_string),
                hit_condition: None,
                log_message: None,
            };
            if let Ok(mut b) = session.bps.lock() {
                b.entry(file.clone()).or_default().push((bp_id.clone(), bp));
            }
            apply(session, &file);
            tx.respond(
                id,
                json!({"breakpointId": bp_id, "locations": [{"scriptId": script_id(&file), "lineNumber": line - 1, "columnNumber": 0}]}),
            );
        }
        "Debugger.removeBreakpoint" => {
            let target = p["breakpointId"].as_str().unwrap_or_default();
            let mut touched = None;
            if let Ok(mut b) = session.bps.lock() {
                for (file, list) in b.iter_mut() {
                    if let Some(i) = list.iter().position(|(i, _)| i == target) {
                        list.remove(i);
                        touched = Some(file.clone());
                    }
                }
            }
            if let Some(f) = touched {
                apply(session, &f);
            }
            tx.respond(id, json!({}));
        }
        "Debugger.setPauseOnExceptions" => {
            debug::set_break_on_throw(p["state"].as_str().is_some_and(|s| s != "none"));
            tx.respond(id, json!({}));
        }
        "Debugger.pause" => {
            debug::pause();
            tx.respond(id, json!({}));
        }
        m
        @ ("Debugger.resume" | "Debugger.stepOver" | "Debugger.stepInto" | "Debugger.stepOut") => {
            let how = match m {
                "Debugger.stepOver" => Resume::StepOver,
                "Debugger.stepInto" => Resume::StepIn,
                "Debugger.stepOut" => Resume::StepOut,
                _ => Resume::Continue,
            };
            tx.respond(id, json!({}));
            tx.event("Debugger.resumed", json!({}));
            debug::resume(how);
        }
        "Debugger.evaluateOnCallFrame" => {
            let frame = p["callFrameId"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            eval_result(tx, id, p["expression"].as_str().unwrap_or_default(), frame);
        }
        "Runtime.evaluate" => eval_result(tx, id, p["expression"].as_str().unwrap_or_default(), 0),
        "Runtime.getProperties" => {
            let vars = match p["objectId"].as_str().unwrap_or_default() {
                "scope" => debug::scope_variables(),
                h => h
                    .strip_prefix("h:")
                    .and_then(|n| n.parse().ok())
                    .map(debug::variables)
                    .unwrap_or_default(),
            };
            tx.respond(id, properties(&vars));
        }
        "Debugger.getPossibleBreakpoints" => tx.respond(id, json!({"locations": []})),
        "Runtime.getIsolateId" => tx.respond(id, json!({"id": "platypus-1"})),
        "Runtime.getHeapUsage" => tx.respond(id, json!({"usedSize": 0, "totalSize": 0})),
        m if m.starts_with("Debugger.")
            || m.starts_with("Runtime.")
            || m.starts_with("Profiler.")
            || m.starts_with("HeapProfiler.")
            || m.starts_with("Network.")
            || m.starts_with("Log.")
            || m.starts_with("NodeWorker.")
            || m.starts_with("Target.") =>
        {
            // Settings the debugger does not use (blackboxing, async stacks, ...).
            tx.respond(id, json!({}));
        }
        other => tx.fail(id, &format!("{other} is not supported")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-API-922: the WebSocket handshake answer is RFC 6455's worked example.
    #[test]
    fn websocket_accept_key_matches_the_rfc_example() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn base64_pads_like_the_standard() {
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
    }
}
