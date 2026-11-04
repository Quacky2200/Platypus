// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A fault in compiled TypeScript is reported (`crash.rs`): a crash report is saved and names the
//! function, also when that function is a local one (not exported). Before, only exported
//! functions were known, and a fault in a local function laid out before them was taken for a
//! driver's: logged, but no report and no dialog. Windows only (the reporter is a vectored
//! exception handler); needs a GPU and runs windowless (`--bench`), which shows no dialog.

#![cfg(windows)]

use std::process::Command;

const GAME: &str = r#"
interface Point { x: number }
function findPoint(n: number): Point | null {
  return n > 1000 ? { x: n } : null;
}
// A null dereference: the program faults here (`!` asserts what is not so).
function crashHere(n: number): number {
  return findPoint(n)!.x;
}
class Crasher extends Component {
  frames = 0;
  override onUpdate(_dt: number) {
    this.frames++;
    if (this.frames === 3) console.log(`value ${crashHere(this.frames)}`);
  }
}
register({ Crasher });
"#;

// M9-API-919
#[test]
fn a_fault_in_a_local_compiled_function_saves_a_report_naming_it() {
    let dir = std::env::temp_dir().join(format!("platypus-crash-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let game = dir.join("main.ts");
    std::fs::write(&game, GAME).unwrap();
    let log = dir.join("run.log");
    let status = Command::new(env!("CARGO_BIN_EXE_platypus-app"))
        .arg(&game)
        .args([
            "--bench", "--warmup", "0", "--frames", "20", "--size", "320x200",
        ])
        .env("PLATYPUS_LOG_FILE", &log)
        .status()
        .unwrap();
    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !status.success(),
        "the game should have crashed: {log_text}"
    );
    assert!(
        log_text.contains("in compiled TypeScript, near crashHere+"),
        "{log_text}"
    );
    let saved = log_text
        .lines()
        .find_map(|l| l.split("crash report saved to ").nth(1))
        .unwrap_or_else(|| panic!("no report saved: {log_text}"))
        .trim()
        .to_string();
    let report = std::fs::read_to_string(&saved).unwrap();
    assert!(report.starts_with("Platypus crash report"), "{report}");
    assert!(report.contains("crashHere"), "{report}");
    // The call stack lists the compiled frames: the faulting function and its caller's hook.
    assert!(report.contains("call stack:"), "{report}");
    assert!(report.contains("Crasher.onUpdate"), "{report}");
    let _ = std::fs::remove_file(&saved);
    let _ = std::fs::remove_dir_all(&dir);
}

const PANICS: &str = r#"
class Late extends Component {}
// Adding a component of a class that was never registered panics in the host, inside the
// function the compiled code called: it cannot unwind, so the process aborts.
new GameObject("x").addComponent(Late);
"#;

// M9-API-920
#[test]
fn a_panic_that_aborts_the_process_saves_a_report_with_the_panic() {
    let dir = std::env::temp_dir().join(format!("platypus-crash-abort-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let game = dir.join("main.ts");
    std::fs::write(&game, PANICS).unwrap();
    let log = dir.join("run.log");
    let status = Command::new(env!("CARGO_BIN_EXE_platypus-app"))
        .arg(&game)
        .args([
            "--bench", "--warmup", "0", "--frames", "20", "--size", "320x200",
        ])
        .env("PLATYPUS_LOG_FILE", &log)
        .status()
        .unwrap();
    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !status.success(),
        "the game should have aborted: {log_text}"
    );
    let saved = log_text
        .lines()
        .find_map(|l| l.split("crash report saved to ").nth(1))
        .unwrap_or_else(|| panic!("no report saved: {log_text}"))
        .trim()
        .to_string();
    let report = std::fs::read_to_string(&saved).unwrap();
    assert!(report.contains("cannot unwind"), "{report}");
    assert!(report.contains("addComponent"), "{report}");
    let _ = std::fs::remove_file(&saved);
    let _ = std::fs::remove_dir_all(&dir);
}

const RECURSES: &str = r#"
function down(n: number): number {
  return down(n + 1) + 1;
}
class Deep extends Component {
  frames = 0;
  override onUpdate(_dt: number) {
    this.frames++;
    if (this.frames === 3) console.log(`depth ${down(0)}`);
  }
}
register({ Deep });
"#;

// M9-API-921
#[test]
fn a_stack_overflow_in_compiled_code_saves_a_report_with_the_recursion() {
    let dir = std::env::temp_dir().join(format!("platypus-crash-overflow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let game = dir.join("main.ts");
    std::fs::write(&game, RECURSES).unwrap();
    let log = dir.join("run.log");
    let status = Command::new(env!("CARGO_BIN_EXE_platypus-app"))
        .arg(&game)
        .args([
            "--bench", "--warmup", "0", "--frames", "20", "--size", "320x200",
        ])
        .env("PLATYPUS_LOG_FILE", &log)
        .status()
        .unwrap();
    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !status.success(),
        "the game should have crashed: {log_text}"
    );
    let saved = log_text
        .lines()
        .find_map(|l| l.split("crash report saved to ").nth(1))
        .unwrap_or_else(|| panic!("no report saved: {log_text}"))
        .trim()
        .to_string();
    let report = std::fs::read_to_string(&saved).unwrap();
    assert!(report.contains("Stack overflow"), "{report}");
    assert!(report.contains("call stack:"), "{report}");
    // The recursion is one counted run, not thousands of lines.
    assert!(report.contains("down at"), "{report}");
    assert!(report.contains("(x"), "{report}");
    assert!(
        report.lines().count() < 120,
        "{} lines",
        report.lines().count()
    );
    let _ = std::fs::remove_file(&saved);
    let _ = std::fs::remove_dir_all(&dir);
}
