// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A crash that is not a Rust panic (an access violation in compiled TypeScript, the runtime or a
//! driver, or heap corruption) otherwise ends the process with an exit code and no log line. On
//! Windows this logs what it can first: the exception, where it happened (and the compiled game
//! function, when the address is in one), the registers, the code there and a backtrace. When the
//! fault is the game's own (in this exe or its compiled code, or heap corruption), it also saves a
//! crash report beside the log and starts the crash dialog's process on it (`crash_dialog.rs`):
//! the dialog never runs in the broken process. `Application.crash` takes the same way.

use super::crash_dialog::{CHILD_ENV, PLAIN_FLAG, REPORT_FLAG};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// The exit code after `Application.crash`.
pub const EXIT_CRASH: i32 = 3;

/// The compiled game's exported functions by address, ascending, to name a fault in JIT code.
static JIT_FUNCS: Mutex<Vec<(usize, String)>> = Mutex::new(Vec::new());

/// Where reports go and whether a dialog shows, set once the game is known.
static REPORTS: OnceLock<Reports> = OnceLock::new();

/// Set by the first report: a fault while reporting, or a second crash, adds nothing.
static REPORTED: AtomicBool = AtomicBool::new(false);

struct Reports {
    dir: PathBuf,
    game: String,
    log: Option<PathBuf>,
    dialog: bool,
}

/// Sets where crash reports are saved (the log's folder), the game's name, the log file and
/// whether the dialog shows (not in screenshot or bench runs, nor in the crash dialog itself).
pub fn set_reports(dir: PathBuf, game: String, log: Option<PathBuf>, dialog: bool) {
    let dialog = dialog && std::env::var_os(CHILD_ENV).is_none();
    let _ = REPORTS.set(Reports {
        dir,
        game,
        log,
        dialog,
    });
}

/// Adds a compiled module's functions (name, address); a reload adds the new module's.
pub fn note_jit_functions(funcs: impl IntoIterator<Item = (String, usize)>) {
    let Ok(mut table) = JIT_FUNCS.lock() else {
        return;
    };
    table.extend(funcs.into_iter().map(|(name, addr)| (addr, name)));
    table.sort_unstable_by_key(|(addr, _)| *addr);
}

/// Machine-code ranges `(start, end, line)` of the compiled game by address, ascending; `line` is
/// a line of the bundled source (the sidecar maps it to a file, see the debug-info research doc).
static JIT_LINES: Mutex<Vec<(usize, usize, u32)>> = Mutex::new(Vec::new());

/// Adds a compiled module's line ranges; a reload adds the new module's.
pub fn note_jit_lines(lines: &[(usize, usize, u32)]) {
    let Ok(mut table) = JIT_LINES.lock() else {
        return;
    };
    table.extend_from_slice(lines);
    table.sort_unstable_by_key(|(a, _, _)| *a);
}

/// The line of the bundled source whose code contains `addr`.
fn jit_line_at(addr: usize) -> Option<u32> {
    let table = JIT_LINES.try_lock().ok()?;
    let i = table
        .partition_point(|(a, _, _)| *a <= addr)
        .checked_sub(1)?;
    let (_, end, line) = table[i];
    (addr < end).then_some(line)
}

/// The hash of the bundled source the running code was compiled from (`platypus_ts::modules::source_hash`).
static SOURCE_HASH: AtomicU64 = AtomicU64::new(0);

/// The game folder, where `build/game.pdebug` is looked for after the one beside the exe.
static GAME_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Notes the source the compiled game came from, to check the debug sidecar against.
pub fn note_source_hash(hash: u64) {
    SOURCE_HASH.store(hash, Ordering::Relaxed);
}

/// Notes the game folder (for `build/game.pdebug`).
pub fn note_game_dir(dir: &Path) {
    let _ = GAME_DIR.set(dir.to_path_buf());
}

/// `game.pdebug`: where each file starts in the bundled source (`platypus-build` writes it).
struct DebugInfo {
    hash: String,
    /// (0-based line of the bundle where the file starts, path).
    starts: Vec<(usize, String)>,
}

impl DebugInfo {
    fn parse(json: &str) -> Option<DebugInfo> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let starts = v["starts"]
            .as_array()?
            .iter()
            .filter_map(|p| Some((p[0].as_u64()? as usize, p[1].as_str()?.to_string())))
            .collect();
        Some(DebugInfo {
            hash: v["hash"].as_str()?.to_string(),
            starts,
        })
    }

    /// The file and line (1-based) that line `line` (1-based) of the bundled source came from.
    fn locate(&self, line: u32) -> Option<(&str, usize)> {
        let at = (line as usize).checked_sub(1)?;
        let (start, file) = self.starts.iter().rev().find(|(s, _)| *s <= at)?;
        Some((file, at - start + 1))
    }
}

/// The sidecar beside the exe (`<exe>.pdebug`), else the game folder's `build/game.pdebug`.
fn load_debug_info() -> Option<DebugInfo> {
    let beside = std::env::current_exe()
        .ok()
        .map(|e| e.with_extension("pdebug"));
    let in_game = GAME_DIR.get().map(|d| d.join("build/game.pdebug"));
    beside
        .into_iter()
        .chain(in_game)
        .find_map(|p| DebugInfo::parse(&std::fs::read_to_string(p).ok()?))
}

/// Where in the TypeScript source a line of the bundled source is: `file:line` with the sidecar
/// from this build, else the bundle line.
fn describe_line(line: u32) -> String {
    match load_debug_info() {
        Some(info) if info.hash == format!("{:016x}", SOURCE_HASH.load(Ordering::Relaxed)) => {
            info.locate(line).map_or_else(
                || format!("bundled source line {line}"),
                |(f, l)| format!("{f}:{l}"),
            )
        }
        Some(_) => format!("bundled source line {line} (the .pdebug file is from another build)"),
        None => format!("bundled source line {line}"),
    }
}

/// The nearest exported game function at or below `addr`, as `name+offset`. Only a hint: a
/// local (not exported) function in between is not in the table.
fn jit_function_at(addr: usize) -> Option<String> {
    let table = JIT_FUNCS.try_lock().ok()?;
    let i = table.partition_point(|(a, _)| *a <= addr).checked_sub(1)?;
    let (start, name) = &table[i];
    // Beyond 1 MB the address is not plausibly in that function.
    (addr - start < 1 << 20).then(|| format!("{name}+{:#x}", addr - start))
}

/// Saves a report (`crash-<seconds>.txt` beside the log) and, unless this run shows no dialogs,
/// starts the crash dialog's process on it. Only the first call of a process does anything.
fn save_and_show(reason: &str, details: &str, plain: bool) {
    if REPORTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(r) = REPORTS.get() else {
        return;
    };
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let path = r.dir.join(format!("crash-{secs}.txt"));
    let log = r
        .log
        .as_ref()
        .map(|l| format!("Log: {}\n", l.display()))
        .unwrap_or_default();
    let text = format!(
        "Platypus crash report\nGame: {}\nReason: {reason}\nTime: {secs} (Unix seconds)\n{log}\n{details}\n",
        r.game
    );
    if let Err(e) = std::fs::write(&path, text) {
        eprintln!(
            "crash: the report could not be saved to {}: {e}",
            path.display()
        );
        return;
    }
    tracing::error!("crash report saved to {}", path.display());
    if r.dialog
        && let Ok(exe) = std::env::current_exe()
    {
        let mut dialog = Command::new(exe);
        // Not the game's stdio: whatever reads the game's output must not wait for the dialog.
        dialog
            .arg(REPORT_FLAG)
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if plain {
            dialog.arg(PLAIN_FLAG);
        }
        if let Err(e) = dialog.spawn() {
            eprintln!("crash: the dialog could not start: {e}");
        }
    }
}

/// `Application.crash(message, plain)`: a report and the dialog, as for an unexpected error, then
/// the process ends with [`EXIT_CRASH`].
fn script_crash(message: &str, plain: bool) {
    let reason = if message.is_empty() {
        "The game called Application.crash()".to_string()
    } else {
        format!("The game called Application.crash: {message}")
    };
    tracing::error!("crash: {reason}");
    let details = format!("{reason}\n\n{}", std::backtrace::Backtrace::force_capture());
    save_and_show(&reason, &details, plain);
    std::process::exit(EXIT_CRASH);
}

/// Installs the reporter (once) and `Application.crash`'s handler.
pub fn install() {
    platypus_engine_api::app::set_crash_hook(script_crash);
    #[cfg(windows)]
    {
        use std::sync::Once;
        use windows_sys::Win32::System::Diagnostics::Debug::AddVectoredExceptionHandler;
        static ONCE: Once = Once::new();
        let _ = own_image();
        // A vectored handler, first in line: an unhandled-exception filter is replaced by
        // libraries that install their own, and was never reached by the crashes this was
        // written for.
        // SAFETY: the handler is a plain function that lives for the whole program.
        ONCE.call_once(|| unsafe {
            AddVectoredExceptionHandler(1, Some(report));
        });
    }
}

/// The address range of this program's own image, worked out once.
#[cfg(windows)]
fn own_image() -> (usize, usize) {
    static RANGE: OnceLock<(usize, usize)> = OnceLock::new();
    *RANGE.get_or_init(|| {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::System::ProcessStatus::{K32GetModuleInformation, MODULEINFO};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: queries about this process's own exe, into a zeroed plain struct.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let mut info: MODULEINFO = std::mem::zeroed();
            if K32GetModuleInformation(
                GetCurrentProcess(),
                module,
                &mut info,
                std::mem::size_of::<MODULEINFO>() as u32,
            ) == 0
            {
                return (0, 0);
            }
            let base = info.lpBaseOfDll as usize;
            (base, base + info.SizeOfImage as usize)
        }
    })
}

/// The general registers at the fault: which one held a bad pointer, and what was in it.
///
/// # Safety
/// `ctx` is null or the context record of the exception being handled.
#[cfg(all(windows, target_arch = "x86_64"))]
unsafe fn registers(ctx: *const windows_sys::Win32::System::Diagnostics::Debug::CONTEXT) -> String {
    // SAFETY: by the contract.
    let Some(c) = (unsafe { ctx.as_ref() }) else {
        return String::new();
    };
    format!(
        "registers: rip {:#x} rsp {:#x} rbp {:#x}\n  rax {:#x} rbx {:#x} rcx {:#x} rdx {:#x}\n  rsi {:#x} rdi {:#x} r8 {:#x} r9 {:#x}\n  r10 {:#x} r11 {:#x} r12 {:#x} r13 {:#x} r14 {:#x} r15 {:#x}",
        c.Rip,
        c.Rsp,
        c.Rbp,
        c.Rax,
        c.Rbx,
        c.Rcx,
        c.Rdx,
        c.Rsi,
        c.Rdi,
        c.R8,
        c.R9,
        c.R10,
        c.R11,
        c.R12,
        c.R13,
        c.R14,
        c.R15
    )
}

/// The TypeScript call stack at the fault, innermost first: the faulting address, then every
/// word of the stack that is a return address into compiled game code (a scan, see
/// `platypus_ts::unwind::stack_words`; it crosses the host's frames between the game's, and may
/// list a stale entry). Needs the line table, so only code it covers shows.
///
/// # Safety
/// `ctx` is null or the context record of the exception being handled, on its own thread.
#[cfg(all(windows, target_arch = "x86_64"))]
unsafe fn game_stack(
    ctx: *const windows_sys::Win32::System::Diagnostics::Debug::CONTEXT,
) -> String {
    // SAFETY: by the contract.
    let Some(c) = (unsafe { ctx.as_ref() }) else {
        return String::new();
    };
    // SAFETY: the handler runs on the faulting thread, whose stack pointer this is.
    let words = unsafe { platypus_ts::unwind::stack_words(c.Rsp as usize) };
    let mut out = Vec::new();
    let mut last = 0;
    for (i, w) in std::iter::once(c.Rip as usize).chain(words).enumerate() {
        // A return address is after the call: look the line up for the call itself.
        let at = if i == 0 { w } else { w.wrapping_sub(1) };
        let Some(line) = jit_line_at(at) else {
            continue;
        };
        if at == last {
            continue;
        }
        last = at;
        let f = jit_function_at(at).unwrap_or_default();
        out.push(format!("  {} ({f} at {})", out.len(), describe_line(line)));
    }
    if out.is_empty() {
        String::new()
    } else {
        format!(
            "call stack:
{}",
            out.join(
                "
"
            )
        )
    }
}

#[cfg(all(windows, not(target_arch = "x86_64")))]
unsafe fn game_stack(_: *const windows_sys::Win32::System::Diagnostics::Debug::CONTEXT) -> String {
    String::new()
}

#[cfg(all(windows, not(target_arch = "x86_64")))]
unsafe fn registers(_: *const windows_sys::Win32::System::Diagnostics::Debug::CONTEXT) -> String {
    String::new()
}

#[cfg(windows)]
unsafe extern "system" fn report(
    info: *mut windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    const CONTINUE_SEARCH: i32 = 0;
    const ACCESS_VIOLATION: u32 = 0xC000_0005;
    const HEAP_CORRUPTION: u32 = 0xC000_0374;
    // SAFETY: the system passes a valid record for the exception being raised.
    let rec = unsafe { &*(*info).ExceptionRecord };
    let code = rec.ExceptionCode as u32;
    // A vectored handler also sees exceptions that are handled later (C++ exceptions, debugger
    // and driver probes): only these two are crashes worth a report.
    if code != ACCESS_VIOLATION && code != HEAP_CORRUPTION {
        return CONTINUE_SEARCH;
    }
    let at = rec.ExceptionAddress as usize;
    // Executing a bad address means the code at `at` is not there to read either.
    let code_readable = !(code == ACCESS_VIOLATION
        && rec.NumberParameters >= 2
        && rec.ExceptionInformation[0] == 8);
    let access = (code == ACCESS_VIOLATION && rec.NumberParameters >= 2).then(|| {
        let what = match rec.ExceptionInformation[0] {
            0 => "reading",
            1 => "writing",
            8 => "executing",
            _ => "accessing",
        };
        format!("{what} {:#x}", rec.ExceptionInformation[1])
    });
    let func = jit_function_at(at);
    let (start, end) = own_image();
    let in_exe = (start..end).contains(&at);
    let thread = std::thread::current();
    // SAFETY: as above, the context of the faulting thread.
    let mut registers = unsafe { registers((*info).ContextRecord) };
    // The code before the fault, to see where the bad address came from (a disassembler reads it).
    if code_readable && at > 96 {
        // SAFETY: the faulting instruction is mapped code (it was fetched), and so are the 96
        // bytes before it in the same function or the one before.
        let bytes = unsafe { std::slice::from_raw_parts((at - 96) as *const u8, 112) };
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        registers.push_str(&format!("\ncode [rip-96, rip+16): {}", hex.join(" ")));
    }
    let what = match (code, &access) {
        (HEAP_CORRUPTION, _) => "Heap corruption".to_string(),
        (_, Some(a)) => format!("Access violation {a}"),
        _ => "Access violation".to_string(),
    };
    let place = match &func {
        Some(f) => match jit_line_at(at) {
            Some(l) => format!(
                " (in compiled TypeScript, near {f}, at {})",
                describe_line(l)
            ),
            None => format!(" (in compiled TypeScript, near {f})"),
        },
        None if in_exe => " (in the game's program)".to_string(),
        None => String::new(),
    };
    let reason = format!("{what} at {at:#x}{place}");
    // SAFETY: as above.
    let stack = unsafe { game_stack((*info).ContextRecord) };
    let message = format!(
        "crash: exception {code:#010x}: {reason} on thread {:?}\n{registers}\n{stack}\n{}",
        thread.name().unwrap_or("<unnamed>"),
        std::backtrace::Backtrace::force_capture()
    );
    // Both: the log file may be the only place a GUI build writes to.
    eprintln!("{message}");
    tracing::error!("{message}");
    // A fault elsewhere (a driver) may be one its code handles: only ours are certainly fatal.
    if code == HEAP_CORRUPTION || func.is_some() || in_exe {
        save_and_show(&reason, &message, false);
    }
    CONTINUE_SEARCH
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-TS-3413
    #[test]
    fn the_debug_sidecar_maps_bundle_lines_to_files() {
        let info = DebugInfo::parse(
            r#"{"format":1,"hash":"00000000000000ff","starts":[[0,"src/a.ts"],[10,"src/main.ts"]]}"#,
        )
        .unwrap();
        assert_eq!(info.hash, "00000000000000ff");
        assert_eq!(info.locate(1), Some(("src/a.ts", 1)));
        assert_eq!(info.locate(10), Some(("src/a.ts", 10)));
        assert_eq!(info.locate(11), Some(("src/main.ts", 1)));
        assert_eq!(info.locate(0), None);
        assert!(DebugInfo::parse("not json").is_none());
    }
}
