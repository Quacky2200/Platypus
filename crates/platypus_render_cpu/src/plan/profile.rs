// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `PLATYPUS_CPU_PROFILE=1`: where a frame's time goes, by kind of step, logged every [`EVERY`] frames (milliseconds a
//! frame, and how many steps of each kind). Off, it costs one flag test a step.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use platypus_render::plan::Step;

/// Frames between reports.
const EVERY: u32 = 120;

/// The kinds of step reported apart.
const KINDS: [&str; 5] = ["clear/copy", "draw", "blur", "composite", "finish"];

pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var_os("PLATYPUS_CPU_PROFILE").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

/// The report's row for `step`.
pub fn kind(step: &Step) -> usize {
    match step {
        Step::Clear { .. } | Step::Copy { .. } => 0,
        Step::Draw { .. } => 1,
        Step::Blur { .. } => 2,
        Step::Composite { .. } => 3,
    }
}

/// The finishing work of a frame (pending clears, a linear canvas's encoding).
pub const FINISH: usize = 4;

#[derive(Default)]
pub struct Profile {
    frames: u32,
    time: [Duration; KINDS.len()],
    steps: [u32; KINDS.len()],
    whole: Duration,
}

impl Profile {
    /// Counts `started.elapsed()` against kind `kind`.
    pub fn add(&mut self, kind: usize, started: Instant) {
        self.time[kind] += started.elapsed();
        self.steps[kind] += 1;
    }

    /// Ends a frame begun at `started`; logs and starts over every [`EVERY`] frames.
    pub fn frame(&mut self, started: Instant) {
        self.whole += started.elapsed();
        self.frames += 1;
        if self.frames < EVERY {
            return;
        }
        let per = |d: Duration| d.as_secs_f64() * 1000.0 / f64::from(self.frames);
        let parts: Vec<String> = KINDS
            .iter()
            .enumerate()
            .map(|(i, k)| {
                format!(
                    "{k} {:.2} ms ({:.1} steps)",
                    per(self.time[i]),
                    f64::from(self.steps[i]) / f64::from(self.frames)
                )
            })
            .collect();
        tracing::info!(
            "cpu plan: {:.2} ms a frame: {}",
            per(self.whole),
            parts.join(", ")
        );
        *self = Profile::default();
    }
}
