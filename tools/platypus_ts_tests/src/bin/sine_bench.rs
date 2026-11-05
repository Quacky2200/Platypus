// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `sine_bench [frames]`: one frame of the sine-circle demo is the curve (361 points a degree,
//! Catmull-Rom smoothed to 1,083 points) flattened to the list `render.stroke` takes. Compared:
//! bun, and the native code `platypus_ts` generated (computing it, then Rust reading the array in
//! place).
//!
//! The second part runs the demo's own class (`bench/sine_demo.ts`, copied verbatim from the
//! commit before `LineRenderer2D`: `SineCircle.points()` building `Vector3` objects with a point
//! every degree) both ways: bun, and the compiled code. One source, so it also shows what a
//! swap-out costs.
use platypus_ts_rt::{Array, link_anchor};
use platypus_ts_tests::sine::*;
use std::process::Command;
use std::time::Instant;

fn bun(frames: usize, dir: &std::path::Path) -> Option<(f64, f64)> {
    let out = Command::new("bun")
        .arg(dir.join("bench/sine_bench.ts"))
        .arg(frames.to_string())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let line = text.lines().last()?;
    let num = |k: &str| -> Option<f64> {
        let i = line.find(&format!("\"{k}\":"))? + k.len() + 3;
        line[i..].split([',', '}']).next()?.parse().ok()
    };
    Some((num("ms")?, num("checksum")?))
}

fn native(frames: usize, read_in_place: bool) -> (f64, f64) {
    let tags = [TAG_pi, TAG_normal, TAG_doubleSine, TAG_doubleCos];
    let mut sum = 0.0;
    let mut engine_side: Vec<f32> = Vec::new();
    let t = Instant::now();
    for f in 0..frames {
        let (ty, r, fr, d, rot, cx, cy) = (
            tags[(f / 50) % 4],
            100 + f % 120,
            10 + f % 70,
            5 + f % 60,
            (f * 3) % 360,
            504 + f % 40,
            364 + f % 30,
        );
        let flat: *mut Array = unsafe {
            ts_sine_curveFlat(
                ty, r as f64, fr as f64, d as f64, rot as f64, cx as f64, cy as f64,
            )
        };
        let n = unsafe { (*flat).len() };
        let slice = unsafe { std::slice::from_raw_parts((*flat).data as *const f64, n) };
        if read_in_place {
            // What the engine would do: read straight from script memory into its own buffer.
            engine_side.clear();
            engine_side.extend(slice.iter().map(|v| *v as f32));
        }
        sum += slice[n - 2] + slice[n - 1] + n as f64;
    }
    (t.elapsed().as_secs_f64() * 1000.0, sum)
}

fn bun_demo(frames: usize, dir: &std::path::Path) -> Option<(f64, f64)> {
    let out = Command::new("bun")
        .arg("--preload")
        .arg(dir.join("bench/platypus_stub.ts"))
        .arg(dir.join("bench/sine_demo_run.ts"))
        .arg(frames.to_string())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let line = text.lines().last()?;
    let num = |k: &str| -> Option<f64> {
        let i = line.find(&format!("\"{k}\":"))? + k.len() + 3;
        line[i..].split([',', '}']).next()?.parse().ok()
    };
    Some((num("ms")?, num("checksum")?))
}

fn native_demo(frames: usize) -> (f64, f64) {
    let t = Instant::now();
    let sum = unsafe { platypus_ts_tests::sd::ts_sd_frames(frames as f64) };
    (t.elapsed().as_secs_f64() * 1000.0, sum)
}

fn main() {
    assert_ne!(link_anchor(), 0);
    platypus_ts_tests::gc_mode_from_env();
    unsafe { ts_sine_init() };
    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    println!(
        "{frames} frames of the sine circle: 361 points a degree, 1,083 after the spline, 2,166 numbers sent"
    );
    let best = |name: &str, f: &mut dyn FnMut() -> Option<(f64, f64)>| {
        let runs: Vec<_> = (0..3).filter_map(|_| f()).collect();
        let Some(&(ms, sum)) = runs.iter().min_by(|a, b| a.0.total_cmp(&b.0)) else {
            println!("{name:>26}: unavailable");
            return;
        };
        println!(
            "{name:>26}: {ms:9.2} ms total  {:8.3} ms/frame  checksum {sum:.4}",
            ms / frames as f64
        );
    };
    best("bun, compute", &mut || bun(frames, dir));
    best("native, compute", &mut || Some(native(frames, false)));
    best("native, compute + copy", &mut || Some(native(frames, true)));

    // The demo's own class, verbatim.
    unsafe { platypus_ts_tests::sd::ts_sd_init() };
    println!();
    println!("the demo's own SineCircle class, verbatim (a Vector3 per point, 1,084 a frame):");
    best("bun", &mut || bun_demo(frames, dir));
    best("native", &mut || Some(native_demo(frames)));
}

// The `ts_rt_host_*` functions the linked programs call.
use platypus_components as _;
