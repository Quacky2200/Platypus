// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What every GLES test shares: one headless context at a time in the process. ANGLE (D3D11)
//! asserts and crashes (`StateManager11::syncState`) when contexts are made and used on parallel
//! test threads, so a test holds the lock for as long as it draws.

use platypus_render_gles::egl::EglContext;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static ONE: Mutex<()> = Mutex::new(());

/// The process-wide GL lock (a panicking test doesn't poison it for the others).
pub fn one() -> MutexGuard<'static, ()> {
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// A headless context and the lock, released together (the context first).
pub struct Headless {
    cx: EglContext,
    _one: MutexGuard<'static, ()>,
}

impl std::ops::Deref for Headless {
    type Target = EglContext;
    fn deref(&self) -> &EglContext {
        &self.cx
    }
}

/// A headless context from ANGLE (`PLATYPUS_ANGLE_DIR`, or the linked-in one); `None`, with a
/// note, where there is no EGL driver.
pub fn headless() -> Option<Headless> {
    let one = one();
    let dir = std::env::var_os("PLATYPUS_ANGLE_DIR").map(PathBuf::from);
    match EglContext::headless(dir.as_deref()) {
        Ok(cx) => Some(Headless { cx, _one: one }),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}
