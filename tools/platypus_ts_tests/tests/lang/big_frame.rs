// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A compiled function with a frame of tens of kilobytes (a top-level table of thousands of
//! rows mirrors every pointer local in its frame) must probe the stack pages it passes. Without
//! probes it jumped past Windows' stack guard page: a start-up crash about one run in five in
//! Zombie Horders (a 37 KB `init`), a silent death with no report in anything bigger. IDs are in
//! TESTS.md (M9-TS-3392).
use web_ui_ts::jit::{JitOptions, compile_and_load};

// M9-TS-3392
#[test]
fn a_function_with_a_huge_frame_runs_on_a_fresh_thread_stack() {
    let _serial = platypus_ts_tests::serial();
    let rows: Vec<String> = (0..6000)
        .map(|i| format!("  {{ a: {i}, b: {}, c: \"r{i}\" }}", i * 2))
        .collect();
    let src = format!(
        "interface Row {{ a: number; b: number; c: string }}\n\
         const TABLE: Row[] = [\n{}\n];\n\
         export function total(): number {{ return TABLE.length + TABLE[5999]!.b; }}\n",
        rows.join(",\n")
    );
    // A new thread has only its first stack page committed: the frame has to grow it.
    let result = std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(move || {
            let game = compile_and_load(
                &src,
                &JitOptions {
                    prefix: "bigf_",
                    struct_base: 2_360_000,
                    ..Default::default()
                },
            )
            .unwrap();
            game.init().unwrap();
            let f: unsafe extern "C" fn() -> f64 =
                unsafe { std::mem::transmute(game.function("total").unwrap()) };
            unsafe { f() }
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(result, 6000.0 + 11998.0);
}
