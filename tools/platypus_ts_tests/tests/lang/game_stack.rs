// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A stack walked from inside an engine call passes through the compiled game's frames in call
//! order (the unwind tables `platypus_ts::unwind` registers). Windows x64 only. IDs are in
//! TESTS.md (M9-TS-3391).
#![cfg(all(windows, target_arch = "x86_64"))]

use platypus_ts::jit::{JitOptions, compile_and_load};
use std::sync::Mutex;

static SEEN: Mutex<Vec<usize>> = Mutex::new(Vec::new());

extern "C" fn probe_here() {
    let marker = 0usize;
    // SAFETY: the address of a local is this thread's stack pointer, near enough.
    *SEEN.lock().unwrap() =
        unsafe { platypus_ts::unwind::stack_words(&marker as *const usize as usize) };
}

// M9-TS-3391
#[test]
fn a_stack_walk_from_an_engine_call_names_the_game_frames_in_order() {
    let _serial = platypus_ts_tests::serial();
    let manifest = r#"{"namespace":"Probe","functions":[{"name":"here","symbol":"platypus_Probe_here","params":[],"returns":"void"}]}"#;
    let src = "export function inner(): void { Probe.here(); }\n\
               export function middle(): void { inner(); }\n\
               export function outer(): void { middle(); }\n";
    let game = compile_and_load(
        src,
        &JitOptions {
            prefix: "stk_",
            struct_base: 2_350_000,
            manifests: &[manifest.to_string()],
            symbols: vec![("platypus_Probe_here", probe_here as *const () as usize)],
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let outer: unsafe extern "C" fn() =
        unsafe { std::mem::transmute(game.function("outer").unwrap()) };
    unsafe { outer() };

    let mut funcs = game.code_symbols();
    funcs.sort_by_key(|(_, a)| *a);
    let name_at = |pc: usize| {
        let i = funcs.partition_point(|(_, a)| *a <= pc).checked_sub(1)?;
        let (n, a) = &funcs[i];
        (pc - a < 4096).then(|| n.trim_start_matches("ts_stk_").to_string())
    };
    let seen = SEEN.lock().unwrap().clone();
    // Return addresses into compiled code only: the line table covers every instruction.
    let table = game.line_table();
    let names: Vec<String> = seen
        .iter()
        .map(|w| w.wrapping_sub(1))
        .filter(|at| table.iter().any(|(a, b, _)| (*a..*b).contains(at)))
        .filter_map(name_at)
        .collect();
    assert_eq!(names, ["inner", "middle", "outer"], "{seen:x?}");
}
