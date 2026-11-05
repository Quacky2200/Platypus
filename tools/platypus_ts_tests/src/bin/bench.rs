// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench [steps]`: step() on a busy state for bun (a JIT stand-in) and the native code
//! `platypus_ts` generated. Both print a checksum; they should agree.
use platypus_ts_rt::link_anchor;
use platypus_ts_tests::sim::*;
use platypus_ts_tests::*;
use std::process::Command;
use std::time::Instant;

fn native(steps: usize) -> (f64, f64, usize) {
    gc_mode_from_env();
    let b = busy_state();
    let t = Instant::now();
    for _ in 0..steps {
        unsafe { ts_step(b.state, b.input, 1.0 / 60.0, b.walls) };
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    (
        ms,
        busy_checksum(b.state),
        platypus_ts_rt::ts_rt_allocated(),
    )
}

fn bun(steps: usize, dir: &std::path::Path) -> Option<(f64, f64)> {
    let out = Command::new("bun")
        .arg(dir.join("bench/bench.ts"))
        .arg(steps.to_string())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json_lite::Json = serde_json_lite::parse(text.lines().last()?)?;
    Some((v.get("ms")?, v.get("checksum")?))
}

/// Just enough JSON for `{"ms":1.5,"checksum":2}`.
mod serde_json_lite {
    pub struct Json(Vec<(String, f64)>);
    impl Json {
        pub fn get(&self, k: &str) -> Option<f64> {
            self.0.iter().find(|(n, _)| n == k).map(|(_, v)| *v)
        }
    }
    pub fn parse(s: &str) -> Option<Json> {
        let mut out = Vec::new();
        for part in s.trim().trim_matches(|c| c == '{' || c == '}').split(',') {
            let (k, v) = part.split_once(':')?;
            if let Ok(n) = v.trim().parse::<f64>() {
                out.push((k.trim().trim_matches('"').to_string(), n));
            }
        }
        Some(Json(out))
    }
}

fn main() {
    assert_ne!(link_anchor(), 0);
    let steps: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    println!("step() x {steps}, 1 player (MP5 auto-fire), 10 zombies, 50 parked bullets");
    // Best of three: the first run pays for page faults and (for bun) JIT warm-up.
    let best = |name: &str, f: &mut dyn FnMut() -> Option<(f64, f64)>| {
        let mut runs = Vec::new();
        for _ in 0..3 {
            if let Some(r) = f() {
                runs.push(r);
            }
        }
        if runs.is_empty() {
            println!("{name:>8}: unavailable");
            return;
        }
        let (ms, sum) = runs
            .iter()
            .cloned()
            .fold((f64::MAX, 0.0), |a, r| if r.0 < a.0 { r } else { a });
        println!(
            "{name:>8}: {ms:9.2} ms total  {:8.3} us/step  checksum {sum:.4}",
            ms * 1000.0 / steps as f64
        );
    };
    best("bun", &mut || bun(steps, dir));
    best("native", &mut || {
        let (ms, sum, _) = native(steps);
        Some((ms, sum))
    });
    let _ = native(steps);
    let g = platypus_ts_rt::gc_stats();
    println!(
        "native GC over its last runs on this thread: {} collections, {:.1} MiB handed out in all, {} objects and {:.0} KiB live after the last one",
        g.collections,
        g.allocated_bytes as f64 / (1 << 20) as f64,
        g.objects,
        g.live_bytes as f64 / 1024.0
    );
}

// The `ts_rt_component_*` functions the linked programs call.
use platypus_components as _;
