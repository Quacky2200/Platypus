// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Windows x64 unwind tables for JIT code. `cranelift-jit` registers none on Windows, so the system
//! cannot walk a stack through compiled game frames: a crash report then shows nothing past the
//! first one. This registers each function's unwind info (`RtlAddFunctionTable`), which makes
//! `RtlVirtualUnwind` (and so the host's TypeScript call stack in `crash.rs`) work.
//!
//! The table's entries are 32-bit offsets from one base address, so the table and the unwind
//! data must lie within 4 GB above the lowest function: they are placed with `VirtualAlloc` at
//! an address searched for upward from the end of the code.

use windows_sys::Win32::System::Diagnostics::Debug::{
    IMAGE_RUNTIME_FUNCTION_ENTRY, RtlAddFunctionTable,
};
use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc};

/// One function: its address, the size of its code and its unwind info (`UnwindInfo::emit`).
pub struct FnUnwind {
    pub start: usize,
    pub size: u32,
    pub info: Vec<u8>,
}

/// Does the system have unwind info for the code at `addr`? (What a stack walk needs.)
pub fn is_registered(addr: usize) -> bool {
    use windows_sys::Win32::System::Diagnostics::Debug::RtlLookupFunctionEntry;
    let mut base = 0u64;
    // SAFETY: a query by address; it reads only the system's tables.
    !unsafe { RtlLookupFunctionEntry(addr as u64, &mut base, std::ptr::null_mut()) }.is_null()
}

/// The words of the calling thread's stack from `rsp` up: a conservative scan for return
/// addresses into compiled code. The OS unwinder is not used because it runs from a crash
/// handler on whatever state the process is in, and it faulted there in testing (the report
/// would be lost); a scan cannot fault and also crosses the host's frames in between. It may
/// list a stale address; a caller keeps only words inside compiled code.
///
/// # Safety
/// `rsp` is the stack pointer of the calling thread, or of a thread stopped in its handler.
pub unsafe fn stack_words(rsp: usize) -> Vec<usize> {
    use windows_sys::Win32::System::Threading::GetCurrentThreadStackLimits;
    let (mut low, mut high) = (0usize, 0usize);
    // SAFETY: fills two integers.
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    // SAFETY: the limits are this thread's own.
    unsafe { stack_words_in(low, high, rsp) }
}

/// [`stack_words`] for a stack with the given limits (`GetCurrentThreadStackLimits` of its
/// thread), read from another thread: a thread that overflowed its stack has none left to
/// report with.
///
/// # Safety
/// `low..high` is a committed-or-reserved stack of a live thread that is not running, and
/// `rsp` was its stack pointer.
pub unsafe fn stack_words_in(low: usize, high: usize, rsp: usize) -> Vec<usize> {
    let from = rsp.max(low).next_multiple_of(8);
    // A bound keeps the report quick: a megabyte is far deeper than a game's call stack.
    let to = high.min(from + (1 << 20));
    let mut out = Vec::new();
    let mut at = from;
    while at + 8 <= to {
        // SAFETY: between the thread's stack limits; below `high` it is committed stack.
        out.push(unsafe { *(at as *const usize) });
        at += 8;
    }
    out
}

const ENTRY: usize = std::mem::size_of::<IMAGE_RUNTIME_FUNCTION_ENTRY>();
const GRANULE: usize = 64 << 10;

/// Registers the functions' unwind info with the system. The memory is kept for the rest of the
/// process (the compiled code is never unloaded either).
pub fn register(mut funcs: Vec<FnUnwind>) -> Result<(), String> {
    funcs.retain(|f| f.size > 0 && !f.info.is_empty());
    if funcs.is_empty() {
        return Ok(());
    }
    funcs.sort_by_key(|f| f.start);
    let base = funcs[0].start;
    let end = funcs
        .iter()
        .map(|f| f.start + f.size as usize)
        .max()
        .unwrap_or(base);
    let table_len = funcs.len() * ENTRY;
    let mut len = table_len;
    for f in &funcs {
        len = len.next_multiple_of(4) + f.info.len();
    }
    let len = len.next_multiple_of(GRANULE);
    // The search stops where an offset from `base` would not fit 32 bits.
    let limit = base.saturating_add(u32::MAX as usize).saturating_sub(len);
    let mut at = end.next_multiple_of(GRANULE);
    let block = loop {
        if at > limit {
            return Err("no memory near the compiled code for its unwind tables".into());
        }
        // SAFETY: a fresh reservation at a chosen address; it fails if the range is taken.
        let p = unsafe {
            VirtualAlloc(
                at as *const _,
                len,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        if !p.is_null() {
            break p as usize;
        }
        at += GRANULE;
    };
    // SAFETY: `block` is `len` writable bytes owned by this function; every write stays inside.
    unsafe {
        let mut data = table_len;
        for (i, f) in funcs.iter().enumerate() {
            data = data.next_multiple_of(4);
            let entry = (block + i * ENTRY) as *mut u32;
            entry.write((f.start - base) as u32);
            entry
                .add(1)
                .write((f.start + f.size as usize - base) as u32);
            entry.add(2).write((block + data - base) as u32);
            std::ptr::copy_nonoverlapping(f.info.as_ptr(), (block + data) as *mut u8, f.info.len());
            data += f.info.len();
        }
        if !RtlAddFunctionTable(
            block as *const IMAGE_RUNTIME_FUNCTION_ENTRY,
            funcs.len() as u32,
            base as u64,
        ) {
            return Err("RtlAddFunctionTable failed".into());
        }
    }
    Ok(())
}
