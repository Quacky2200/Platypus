// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `ui_bench [rows]`: a UI app (a list of keyed rows: mount, change one row, change every row,
//! reverse), the compiled TSX (`bench/ui_perf.ts`) rendered by the native reconciler
//! (`platypus_ui_native`) into a real `UiTree`. The time includes building the descriptions in
//! compiled code, the diff and applying the ops to the tree.
use platypus_ts_rt::link_anchor;
use platypus_ts_tests::uip::*;
use platypus_ui::{ROOT, UiTree};
use platypus_ui_native::Runtime;
use std::time::Instant;

const PHASES: [&str; 4] = ["mount", "change one row", "change every row", "reverse"];

/// (rev, tick, all) of each phase.
const STEPS: [(bool, f64, bool); 4] = [
    (false, 0.0, false),
    (false, 1.0, false),
    (false, 2.0, true),
    (true, 2.0, true),
];

fn native_run(n: usize) -> ([f64; 4], usize) {
    let mut tree = UiTree::new(1000.0, 1000.0);
    let mut rt = Runtime::new(1 << 30);
    let root = rt.create_root(ROOT);
    let mut times = [0.0; 4];
    for (i, (rev, tick, all)) in STEPS.iter().enumerate() {
        let t = Instant::now();
        let h = unsafe { ts_uip_perfApp(n as f64, *rev, *tick, *all) };
        let el = platypus_ts_ui::element_from(h);
        let errors = rt.render(root, el, &mut tree);
        assert!(errors.is_empty(), "{errors:?}");
        times[i] = t.elapsed().as_secs_f64() * 1000.0;
    }
    (times, rt.ops_applied())
}

fn main() {
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_ui::link_anchor(), 0);
    platypus_ts_tests::gc_mode_from_env();
    unsafe { ts_uip_init() };
    let rows: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    println!("a list of {rows} keyed rows rendered into a real UiTree (best of 7, ms):");
    let mut best = [f64::MAX; 4];
    let mut ops = 0;
    for _ in 0..7 {
        let (t, o) = native_run(rows);
        ops = o;
        for i in 0..4 {
            best[i] = best[i].min(t[i]);
        }
    }
    for i in 0..4 {
        println!("{:>18} {:>11.3}", PHASES[i], best[i]);
    }
    println!("{:>18} {:>11}", "ops applied", ops);
}

// The `ts_rt_host_*` functions the linked programs call.
use platypus_components as _;
