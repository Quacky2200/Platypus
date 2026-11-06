// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The JIT's line table (`Game::line_table`): compiled code maps back to the lines of the source
//! it came from, which a crash report prints. IDs are in TESTS.md (M9-TS-3389).
use web_ui_ts::jit::{JitOptions, compile_and_load};

// M9-TS-3389
#[test]
fn compiled_code_maps_to_source_lines() {
    let _serial = platypus_ts_tests::serial();
    let src =
        "export function f(a: number): number {\n  let x = a * 2;\n  x = x + 1;\n  return x;\n}\n";
    let game = compile_and_load(
        src,
        &JitOptions {
            prefix: "dbg_",
            struct_base: 2_340_000,
            ..Default::default()
        },
    )
    .unwrap();
    let table = game.line_table();
    assert!(!table.is_empty());
    assert!(table.windows(2).all(|w| w[0].0 <= w[1].0), "ascending");
    let lines: std::collections::BTreeSet<u32> = table.iter().map(|(_, _, l)| *l).collect();
    for want in [2, 3, 4] {
        assert!(lines.contains(&want), "line {want} in {lines:?}");
    }
    // M9-TS-3390: the system can unwind through the function (its table entry is registered).
    #[cfg(windows)]
    assert!(web_ui_ts::unwind::is_registered(
        game.function("f").unwrap()
    ));
    // Every range lies inside the compiled function.
    let f = game.function("f").unwrap();
    assert!(table.iter().any(|(a, b, _)| *a >= f && a < b));
}
