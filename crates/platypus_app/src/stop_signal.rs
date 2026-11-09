// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A stop request from outside the program (Ctrl+C, Ctrl+Break, the console closing, SIGTERM), for
//! a headless server to end its loop and say goodbye to its players instead of just vanishing
//! (`headless.rs`). The handler only sets a flag; the loop reads it.

use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether the program was asked to stop.
pub(crate) fn requested() -> bool {
    REQUESTED.load(Ordering::Relaxed)
}

/// Asks the program to stop, as the OS would (for tests).
#[cfg(test)]
pub(crate) fn request() {
    REQUESTED.store(true, Ordering::Relaxed);
}

#[cfg(windows)]
mod os {
    use super::REQUESTED;
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, SetConsoleCtrlHandler,
    };

    unsafe extern "system" fn handler(kind: u32) -> i32 {
        match kind {
            CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT => {
                REQUESTED.store(true, Ordering::Relaxed);
                // Handled: the loop ends the process itself.
                1
            }
            _ => 0,
        }
    }

    pub(super) fn install() {
        // SAFETY: `handler` is a plain function that only stores to an atomic.
        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}

#[cfg(unix)]
mod os {
    use super::REQUESTED;
    use std::sync::atomic::Ordering;

    extern "C" fn handler(_: i32) {
        REQUESTED.store(true, Ordering::Relaxed);
    }

    pub(super) fn install() {
        // SAFETY: `handler` is async-signal-safe: it only stores to an atomic.
        unsafe {
            libc::signal(libc::SIGINT, handler as usize);
            libc::signal(libc::SIGTERM, handler as usize);
        }
    }
}

/// Starts listening for stop requests (a second call does nothing new).
pub(crate) fn install() {
    os::install();
}

#[cfg(test)]
mod tests {
    use super::*;

    // M8-HLS-04
    #[test]
    fn installing_the_handler_is_harmless_and_a_request_is_seen() {
        install();
        install();
        assert!(!requested());
        request();
        assert!(requested());
        REQUESTED.store(false, Ordering::Relaxed);
    }
}
