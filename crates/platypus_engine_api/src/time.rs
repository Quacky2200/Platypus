// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Time`: game time, scaled by `timeScale`. The host calls [`advance`] once per frame with the
//! real frame length; scripts read the rest.

use std::cell::Cell;
use ts_api::ts_api;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeState {
    pub time: f64,
    pub unscaled_time: f64,
    pub delta_time: f64,
    pub unscaled_delta_time: f64,
    pub frame_count: u32,
    pub time_scale: f64,
}

const START: TimeState = TimeState {
    time: 0.0,
    unscaled_time: 0.0,
    delta_time: 0.0,
    unscaled_delta_time: 0.0,
    frame_count: 0,
    time_scale: 1.0,
};

thread_local! {
    static STATE: Cell<TimeState> = const { Cell::new(START) };
    /// When the thread first looked at the clock: `realtimeSinceStartup` counts from here.
    static STARTED: std::time::Instant = std::time::Instant::now();
}

/// Starts a frame `unscaled_dt` seconds long: `deltaTime` is that times `timeScale`.
pub fn advance(unscaled_dt: f64) {
    STATE.with(|s| {
        let mut t = s.get();
        t.unscaled_delta_time = unscaled_dt;
        t.delta_time = unscaled_dt * t.time_scale;
        t.unscaled_time += unscaled_dt;
        t.time += t.delta_time;
        t.frame_count += 1;
        s.set(t);
    });
    // The engine tick also starts the input frame (`input.ts` listens to it).
    crate::input::tick();
}

pub fn state() -> TimeState {
    STATE.with(Cell::get)
}

/// Puts back what a reload saved (the clock keeps running across a code reload).
pub fn restore(time: f64, unscaled_time: f64, frame_count: u32, time_scale: f64) {
    STATE.with(|s| {
        let mut t = s.get();
        t.time = time;
        t.unscaled_time = unscaled_time;
        t.frame_count = frame_count;
        t.time_scale = time_scale;
        s.set(t);
    });
}

/// Back to the start (a new game, or a test).
pub fn reset() {
    STATE.with(|s| s.set(START));
}

#[ts_api(namespace = "Time")]
pub mod time_api {
    use super::{STARTED, STATE};

    /// Seconds of game time since the start (scaled by `timeScale`).
    #[property]
    pub fn time() -> f64 {
        STATE.with(|s| s.get().time)
    }

    /// Seconds since the start, ignoring `timeScale`.
    #[property]
    pub fn unscaled_time() -> f64 {
        STATE.with(|s| s.get().unscaled_time)
    }

    /// This frame's length in seconds, scaled by `timeScale` (what `onUpdate` gets).
    #[property]
    pub fn delta_time() -> f64 {
        STATE.with(|s| s.get().delta_time)
    }

    #[property]
    pub fn unscaled_delta_time() -> f64 {
        STATE.with(|s| s.get().unscaled_delta_time)
    }

    /// The fixed tick length (1/60 s): what `onFixedUpdate` gets.
    #[property]
    pub fn fixed_delta_time() -> f64 {
        1.0 / 60.0
    }

    /// Wall-clock seconds since the game started (the first look at the clock).
    #[property]
    pub fn realtime_since_startup() -> f64 {
        STARTED.with(|t| t.elapsed().as_secs_f64())
    }

    /// Frames run so far.
    #[property]
    pub fn frame_count() -> u32 {
        STATE.with(|s| s.get().frame_count)
    }

    /// Speed of game time: 0 pauses `deltaTime`, 0.5 is slow motion.
    #[property]
    pub fn time_scale() -> f64 {
        STATE.with(|s| s.get().time_scale)
    }

    pub fn set_time_scale(scale: f64) {
        STATE.with(|s| {
            let mut t = s.get();
            t.time_scale = if scale.is_finite() && scale >= 0.0 {
                scale
            } else {
                1.0
            };
            s.set(t);
        });
    }
}
