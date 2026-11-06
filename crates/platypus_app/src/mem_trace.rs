// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `PLATYPUS_MEM_TRACE=1`: logs the process's memory at every start-up mark (the compiler's
//! phases, the host's stages, the window, the renderer, the first frames), to find what a
//! start-up stage costs (`docs/rendering.md`, "Memory"). Windows only; elsewhere it logs nothing.

use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
/// `PLATYPUS_MEM_TRACE=regions`: also where the private bytes are, once the frames run.
static REGIONS: AtomicBool = AtomicBool::new(false);

/// Turns the trace on when `PLATYPUS_MEM_TRACE` is set (not `0`).
pub fn install() {
    let v = std::env::var("PLATYPUS_MEM_TRACE").unwrap_or_default();
    if !v.is_empty() && v != "0" {
        ON.store(true, Ordering::Relaxed);
        REGIONS.store(v == "regions", Ordering::Relaxed);
        web_ui_ts::timing::set_mark_hook(mark);
        mark("process start");
    }
}

/// Is the trace on?
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Logs the memory now, after `stage`.
pub fn mark(stage: &'static str) {
    if !on() {
        return;
    }
    if let Some(m) = sample() {
        // stderr as well as the log: the trace starts before the log does.
        let line = format!(
            "mem: {:>7.1} MB private {:>7.1} MB working set {:>7.1} MB private working set  after {stage}",
            m.private, m.working_set, m.private_working_set
        );
        eprintln!("{line}");
        tracing::info!("{line}");
    }
}

/// With `PLATYPUS_MEM_TRACE=regions`, once: the committed private memory by heap and by
/// allocation (the largest), to tell our heap from a driver's.
pub fn regions_once() {
    if REGIONS.swap(false, Ordering::Relaxed) {
        for line in regions() {
            eprintln!("mem: {line}");
        }
    }
}

#[cfg(windows)]
fn regions() -> Vec<String> {
    use windows_sys::Win32::System::Memory::{
        GetProcessHeaps, HEAP_SUMMARY, HeapSummary, MEM_COMMIT, MEM_PRIVATE,
        MEMORY_BASIC_INFORMATION, VirtualQuery,
    };
    let mb = |b: usize| b as f64 / (1024.0 * 1024.0);
    let mut out = Vec::new();
    let mut heaps = vec![std::ptr::null_mut(); 256];
    // SAFETY: the buffer holds as many handles as it says.
    let n = unsafe { GetProcessHeaps(heaps.len() as u32, heaps.as_mut_ptr()) } as usize;
    heaps.truncate(n.min(heaps.len()));
    let mut heap_total = 0;
    for (i, h) in heaps.iter().enumerate() {
        let mut s = HEAP_SUMMARY {
            cb: std::mem::size_of::<HEAP_SUMMARY>() as u32,
            cbAllocated: 0,
            cbCommitted: 0,
            cbReserved: 0,
            cbMaxReserve: 0,
        };
        // SAFETY: a heap handle of this process and a summary of the size given.
        if unsafe { HeapSummary(*h, 0, &mut s) } == 0 {
            continue;
        }
        heap_total += s.cbCommitted;
        if s.cbCommitted > 1 << 20 {
            out.push(format!(
                "heap {i} ({h:?}{}): {:.1} MB committed, {:.1} MB in use",
                if i == 0 { ", the process heap" } else { "" },
                mb(s.cbCommitted),
                mb(s.cbAllocated)
            ));
        }
    }
    // Committed private pages by allocation.
    let mut by_base: std::collections::HashMap<usize, usize> = Default::default();
    let mut at = 0usize;
    let mut total = 0;
    loop {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: a query of this process's address space into a buffer of the size given.
        let got = unsafe {
            VirtualQuery(
                at as *const _,
                &mut mbi,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if got == 0 {
            break;
        }
        if mbi.State == MEM_COMMIT && mbi.Type == MEM_PRIVATE {
            *by_base.entry(mbi.AllocationBase as usize).or_default() += mbi.RegionSize;
            total += mbi.RegionSize;
        }
        let next = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        if next <= at {
            break;
        }
        at = next;
    }
    out.push(format!(
        "{:.1} MB committed private pages, {:.1} MB of them in {} heaps",
        mb(total),
        mb(heap_total),
        heaps.len()
    ));
    let mut v: Vec<_> = by_base.into_iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (base, size) in v.into_iter().take(16) {
        let heap = heaps.iter().any(|h| *h as usize == base);
        out.push(format!(
            "  {:.1} MB at {base:#x}{}",
            mb(size),
            if heap {
                " (a heap's first segment)"
            } else {
                ""
            }
        ));
    }
    out
}

#[cfg(not(windows))]
fn regions() -> Vec<String> {
    Vec::new()
}

/// The process's memory, in MB.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// Committed private bytes (Process Explorer's "Private Bytes", `PrivateMemorySize64`).
    pub private: f64,
    /// Resident pages, shared ones (the exe, DLLs) included.
    pub working_set: f64,
    /// Resident private pages: Task Manager's "Memory" column.
    pub private_working_set: f64,
}

#[cfg(windows)]
pub fn sample() -> Option<Sample> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        ..Default::default()
    };
    // SAFETY: EX2 starts with the plain counters and `cb` says its size; the handle is this
    // process's pseudo-handle.
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut c).cast::<PROCESS_MEMORY_COUNTERS>(),
            c.cb,
        )
    };
    let mb = |b: usize| b as f64 / (1024.0 * 1024.0);
    (ok != 0).then(|| Sample {
        private: mb(c.PrivateUsage),
        working_set: mb(c.WorkingSetSize),
        private_working_set: mb(c.PrivateWorkingSetSize),
    })
}

#[cfg(not(windows))]
pub fn sample() -> Option<Sample> {
    None
}
