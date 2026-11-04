// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A Debug Adapter Protocol session against the real host (`dap.rs`): the game waits for
//! `configurationDone`, stops at the breakpoint the editor set, steps, and runs to the end after
//! the editor disconnects. Windows only like `crash_report.rs` (windowless `--bench` run, needs
//! a GPU).

#![cfg(windows)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

// Lines are those of this source (1-based).
const GAME: &str = "\
class Counter extends Component {
  n = 0;
  override onUpdate(_dt: number) {
    this.n += 1;
    const doubled = this.n * 2;
    if (doubled > 100000) console.log('big');
  }
}
register({ Counter });
";

struct Client {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    seq: i64,
    events: Vec<Value>,
}

impl Client {
    fn send(&mut self, command: &str, arguments: Value) -> Value {
        self.seq += 1;
        let body =
            json!({"seq": self.seq, "type": "request", "command": command, "arguments": arguments})
                .to_string();
        write!(self.stream, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        loop {
            let m = self.read();
            if m["type"] == "response" && m["request_seq"] == self.seq {
                assert_eq!(m["success"], true, "{command}: {m}");
                return m;
            }
            self.events.push(m);
        }
    }

    fn read(&mut self) -> Value {
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            assert!(
                self.reader.read_line(&mut line).unwrap() > 0,
                "connection closed"
            );
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length:") {
                len = n.trim().parse().unwrap();
            }
        }
        let mut body = vec![0u8; len];
        self.reader.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    /// The next `stopped` event (one already read while waiting for a response counts).
    fn stopped(&mut self) -> Value {
        loop {
            if let Some(i) = self.events.iter().position(|e| e["event"] == "stopped") {
                return self.events.remove(i);
            }
            let m = self.read();
            self.events.push(m);
        }
    }

    fn top_line(&mut self) -> i64 {
        let st = self.send("stackTrace", json!({"threadId": 1}));
        st["body"]["stackFrames"][0]["line"].as_i64().unwrap()
    }
}

// M9-TS-3399
#[test]
fn an_editor_sets_a_breakpoint_steps_and_lets_the_game_finish() {
    let dir = std::env::temp_dir().join(format!("platypus-dap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let game = dir.join("main.ts");
    std::fs::write(&game, GAME).unwrap();
    let mut host = Command::new(env!("CARGO_BIN_EXE_platypus-app"))
        .arg(&game)
        .args([
            "--bench",
            "--warmup",
            "0",
            "--frames",
            "30",
            "--size",
            "320x200",
            "--dap-port",
            "0",
        ])
        .env("PLATYPUS_LOG_FILE", dir.join("run.log"))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The host says which port it listens on, then waits for `configurationDone`.
    let mut err = BufReader::new(host.stderr.take().unwrap());
    let port: u16 = loop {
        let mut line = String::new();
        assert!(
            err.read_line(&mut line).unwrap() > 0,
            "the host ended early"
        );
        if line.contains("DAP server listening")
            && let Some((_, p)) = line.trim().rsplit_once("127.0.0.1:")
        {
            break p.parse().unwrap();
        }
    };
    // Keep draining stderr so the host never blocks on a full pipe.
    std::thread::spawn(move || for _ in err.lines() {});
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    let mut c = Client {
        reader: BufReader::new(stream.try_clone().unwrap()),
        stream,
        seq: 0,
        events: Vec::new(),
    };
    let init = c.send("initialize", json!({"adapterID": "platypus"}));
    assert_eq!(init["body"]["supportsConfigurationDoneRequest"], true);
    c.send("attach", json!({}));
    let set = c.send(
        "setBreakpoints",
        json!({"source": {"path": game.to_string_lossy()}, "breakpoints": [{"line": 4}]}),
    );
    assert_eq!(set["body"]["breakpoints"][0]["verified"], true, "{set}");
    c.send("configurationDone", json!({}));

    let s = c.stopped();
    assert_eq!(s["body"]["reason"], "breakpoint", "{s}");
    assert_eq!(c.top_line(), 4);
    c.send("next", json!({"threadId": 1}));
    let s = c.stopped();
    assert_eq!(s["body"]["reason"], "step", "{s}");
    assert_eq!(c.top_line(), 5);
    c.send("stepIn", json!({"threadId": 1}));
    c.stopped();
    assert_eq!(c.top_line(), 6);
    c.send("disconnect", json!({}));
    drop(c);
    // With the debugger gone the bench finishes its frames and exits cleanly.
    let status = host.wait().unwrap();
    assert!(status.success(), "{status:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
