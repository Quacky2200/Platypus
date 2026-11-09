// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `platypus-app <game> --headless`: the game without a window, GPU, renderer or audio device, for
//! dedicated servers and tests (M8.a, `docs/research/multiplayer.md` §7).
//!
//! Like `--bench` it runs on the fake platform (no OS window, secondary windows virtual), but it
//! never makes a graphics device: `App::frame` skips drawing when there is none. Scripts, scene,
//! physics, timers and `Application.quit` run as in a normal run, at a paced loop rate instead of
//! the display's.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use platypus_platform::{FakePlatform, WindowSpec};

use super::App;
use super::stop_signal;

/// The command line of a headless run.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HeadlessArgs {
    /// `--tick-rate N`: loop iterations per second (default 60). Fixed-step physics and
    /// `onFixedUpdate` keep their own 60 Hz clock; this is how often the game's frame runs.
    pub rate: f64,
}

impl Default for HeadlessArgs {
    fn default() -> Self {
        HeadlessArgs { rate: 60.0 }
    }
}

/// Flags that belong to a headless run (any of them turns it on).
pub(crate) fn is_flag(arg: &str) -> bool {
    matches!(arg, "--headless" | "--tick-rate")
}

impl HeadlessArgs {
    /// Reads one flag (and its value from `rest`).
    pub(crate) fn flag(
        &mut self,
        flag: &str,
        rest: &mut impl Iterator<Item = std::ffi::OsString>,
    ) -> Result<()> {
        match flag {
            "--headless" => {}
            "--tick-rate" => {
                let v = rest.next().context("--tick-rate needs a number")?;
                let rate: f64 = v
                    .to_str()
                    .and_then(|s| s.parse().ok())
                    .context("--tick-rate needs a number")?;
                if !(1.0..=1000.0).contains(&rate) {
                    bail!("--tick-rate must be between 1 and 1000");
                }
                self.rate = rate;
            }
            _ => bail!("unknown headless flag {flag}"),
        }
        Ok(())
    }
}

/// Runs `app` until it quits (`Application.quit`) or `frames` loop iterations have run.
pub(crate) fn run(app: App, args: &HeadlessArgs, frames: Option<u32>, title: &str) -> Result<()> {
    let main = WindowSpec::main(title, 1280.0, 720.0);
    let mut p = FakePlatform::with_app_main(app, main, 1.0);
    let interval = Duration::from_secs_f64(1.0 / args.rate);
    tracing::info!(
        "headless: no window or GPU, {} iterations a second{}",
        args.rate,
        frames.map_or(String::new(), |n| format!(", {n} of them"))
    );
    // Ctrl+C or a terminate request ends the loop; `main` then closes the network (players are told).
    stop_signal::install();
    let mut next = Instant::now();
    let mut done = 0u32;
    while !p.exited() && frames.is_none_or(|n| done < n) && !stop_signal::requested() {
        p.step();
        done += 1;
        next += interval;
        pace(&mut next, interval);
    }
    if stop_signal::requested() {
        tracing::info!("headless: stop requested");
    }
    tracing::info!("headless: stopped after {done} iterations");
    Ok(())
}

/// Waits until `next`: a sleep for most of it, a spin for the last millisecond (the OS sleeps
/// longer than asked). A loop that fell behind skips the missed deadlines rather than running
/// flat out to catch up.
fn pace(next: &mut Instant, interval: Duration) {
    const SPIN: Duration = Duration::from_millis(1);
    let now = Instant::now();
    if *next <= now {
        if now - *next > interval * 4 {
            *next = now;
        }
        return;
    }
    let wait = *next - now;
    if wait > SPIN {
        std::thread::sleep(wait - SPIN);
    }
    while Instant::now() < *next {
        std::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(list: &[&str]) -> Result<HeadlessArgs> {
        let mut args = HeadlessArgs::default();
        let mut it = list.iter().map(std::ffi::OsString::from);
        while let Some(f) = it.next() {
            args.flag(f.to_str().unwrap(), &mut it)?;
        }
        Ok(args)
    }

    // M8-HLS-01
    #[test]
    fn headless_defaults_to_sixty_iterations_a_second() {
        assert_eq!(flags(&["--headless"]).unwrap().rate, 60.0);
    }

    // M8-HLS-01
    #[test]
    fn tick_rate_is_read_and_checked() {
        assert_eq!(
            flags(&["--headless", "--tick-rate", "30"]).unwrap().rate,
            30.0
        );
        assert!(flags(&["--tick-rate", "0"]).is_err());
        assert!(flags(&["--tick-rate", "abc"]).is_err());
        assert!(flags(&["--tick-rate"]).is_err());
    }

    // M8-HLS-02
    #[test]
    fn pacing_waits_for_the_deadline_and_skips_missed_ones() {
        let interval = Duration::from_millis(10);
        let start = Instant::now();
        let mut next = start + interval;
        pace(&mut next, interval);
        assert!(start.elapsed() >= interval);

        // Far behind: the deadline moves to now instead of a burst of catch-up iterations.
        let mut late = Instant::now() - interval * 20;
        pace(&mut late, interval);
        assert!(late.elapsed() < interval);
    }
}
