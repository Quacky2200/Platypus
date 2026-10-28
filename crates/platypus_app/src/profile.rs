// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `PLATYPUS_PROFILE=1`: every 120 frames, logs what a frame costs on the CPU (scripts, the draw
//! list, building and submitting the render) next to how long frames really took (average and
//! worst), so a slow window can be told apart: CPU work, or waiting on the GPU or vsync.

use std::time::{Duration, Instant};

/// Whether `PLATYPUS_PROFILE` is set (read once).
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PLATYPUS_PROFILE").is_some())
}

#[derive(Default)]
pub struct Profile {
    frames: u32,
    tick: Duration,
    draw: Duration,
    render: Duration,
    commands: usize,
    last: Option<Instant>,
    interval: Duration,
    worst: Duration,
}

const WINDOW: u32 = 120;

impl Profile {
    /// One frame done: `tick` is scripts and UI updates, `draw` the draw-list build, `render`
    /// recording, submitting and presenting (it blocks on vsync when the GPU is ahead).
    pub fn record(&mut self, tick: Duration, draw: Duration, render: Duration, commands: usize) {
        let now = Instant::now();
        if let Some(last) = self.last {
            let dt = now - last;
            self.interval += dt;
            self.worst = self.worst.max(dt);
        }
        self.last = Some(now);
        self.frames += 1;
        self.tick += tick;
        self.draw += draw;
        self.render += render;
        self.commands = commands;
        if self.frames == WINDOW {
            let ms = |d: Duration| d.as_secs_f64() * 1e3 / f64::from(WINDOW);
            tracing::info!(
                "PROFILE frame {:.2} ms (worst {:.1}), tick {:.2}, draw list {:.2} ({} commands), render + present {:.2}",
                ms(self.interval),
                self.worst.as_secs_f64() * 1e3,
                ms(self.tick),
                ms(self.draw),
                self.commands,
                ms(self.render),
            );
            *self = Profile {
                last: self.last,
                ..Profile::default()
            };
        }
    }
}
