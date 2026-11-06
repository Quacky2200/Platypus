// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Top-level `await` across modules: a module that imports one with a top-level `await` runs
//! after it has finished, and a module that does not depend on it runs meanwhile (ECMAScript's
//! order). The cases of `bench/tla_modules_cases.txt` are bundled, run in the normal, stress GC
//! and collection-off modes and compared with bun running the files as real modules
//! (`bench/gen_tla_modules_expected.ts`). IDs are in TESTS.md.
use serde_json::Value;
use std::collections::HashMap;
use web_ui_rt::{
    StartupState, Str, link_anchor, startup_state, ts_rt_gc_enable, ts_rt_gc_stress, ts_rt_run_loop,
};
use web_ui_ts::jit::{JitOptions, compile_and_load};

const CASES: &str = include_str!("../../bench/tla_modules_cases.txt");

/// The files of case `name`.
fn files_of(name: &str) -> HashMap<String, String> {
    let mut parts = CASES.split("//## ");
    parts.next();
    for p in parts {
        let (head, body) = p.split_once('\n').unwrap();
        if head.trim() != name {
            continue;
        }
        let mut files = HashMap::new();
        let mut pieces = body.split("//@ ");
        pieces.next();
        for f in pieces {
            let (path, text) = f.split_once('\n').unwrap();
            files.insert(path.trim().to_string(), text.to_string());
        }
        return files;
    }
    panic!("no case {name}")
}

fn expected(name: &str) -> String {
    let v: Value =
        serde_json::from_str(include_str!("../../bench/tla_modules_expected.json")).unwrap();
    v[name].as_str().unwrap().to_string()
}

fn bundle_of(files: &HashMap<String, String>) -> web_ui_ts::modules::Bundle {
    web_ui_ts::modules::bundle_with("main.ts", &|p| files.get(p).cloned()).unwrap()
}

/// Runs the program (init, then the event loop) in the three GC modes; the results must agree.
fn run_files(files: &HashMap<String, String>, prefix: &'static str, base: u32) -> String {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let game = compile_and_load(
        &bundle_of(files).source,
        &JitOptions {
            prefix,
            struct_base: base,
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let mut out: Vec<String> = Vec::new();
    for mode in 0..3 {
        match mode {
            1 => ts_rt_gc_stress(true),
            2 => {
                ts_rt_gc_stress(false);
                ts_rt_gc_enable(false);
            }
            _ => {}
        }
        game.init().unwrap();
        ts_rt_run_loop();
        assert_eq!(startup_state(), StartupState::Done);
        let result: unsafe extern "C" fn() -> *mut u8 =
            unsafe { std::mem::transmute(game.function("result").unwrap()) };
        let p = unsafe { result() };
        out.push(String::from_utf16_lossy(
            &unsafe { Str::from_raw(p as *const Str) }.to_utf16(),
        ));
    }
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(true);
    assert!(
        out.iter().all(|r| *r == out[0]),
        "GC modes disagree: {out:?}"
    );
    out.remove(0)
}

macro_rules! bun_tests {
    ($( $id:literal $fn:ident $name:literal $prefix:literal $base:literal; )*) => {$(
        #[doc = concat!("M9-TS-", stringify!($id))]
        #[test]
        fn $fn() {
            assert_eq!(run_files(&files_of($name), $prefix, $base), expected($name), "{}", $name);
        }
    )*};
}

bun_tests! {
    3202 an_importer_waits_for_the_module_with_a_top_level_await "importer_waits_for_the_awaiting_module" "tm0_" 910000;
    3203 a_sibling_runs_while_another_module_is_pending "a_sibling_runs_while_the_other_is_pending" "tm1_" 911000;
    3204 two_awaiting_siblings_overlap "two_awaiting_siblings_overlap" "tm2_" 912000;
    3205 a_chain_of_importers_waits_all_the_way_up "a_chain_waits_all_the_way_up" "tm3_" 913000;
    3206 modules_without_an_await_run_in_import_order "modules_without_an_await_keep_their_order" "tm4_" 914000;
    3207 the_entry_can_await_after_awaiting_imports "an_await_in_the_entry_after_awaiting_imports" "tm5_" 915000;
}

// M9-TS-3208
//
// The bundle only carries module markers when some module awaits at its top level (the source of
// every other program is unchanged).
#[test]
fn module_markers_exist_only_when_a_module_awaits() {
    let files = files_of("modules_without_an_await_keep_their_order");
    assert!(!bundle_of(&files).source.contains("__platypus_module"));
    let files = files_of("importer_waits_for_the_awaiting_module");
    assert!(bundle_of(&files).source.contains("__platypus_module"));
}

// M9-TS-3209
//
// A rejected top-level `await` fails the startup and names the module and the error; the importer
// never runs.
#[test]
fn a_rejected_top_level_await_fails_the_startup_naming_the_module() {
    let _serial = platypus_ts_tests::serial();
    let files: HashMap<String, String> = [
        (
            "bad.ts".to_string(),
            "export const v: number = await Promise.reject(new Error(\"no network\"));\n"
                .to_string(),
        ),
        (
            "main.ts".to_string(),
            "import { v } from \"./bad\";\nlet ran = false;\nran = true;\nexport function result(): string { return String(ran) + v; }\n"
                .to_string(),
        ),
    ]
    .into();
    let game = compile_and_load(
        &bundle_of(&files).source,
        &JitOptions {
            prefix: "tm6_",
            struct_base: 916_000,
            ..Default::default()
        },
    )
    .unwrap();
    thread_local! {
        static LOG: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    extern "C" fn hook(level: i32, text: *const u8, len: usize) {
        let s = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, len) });
        LOG.with(|l| l.borrow_mut().push(format!("{level}:{s}")));
    }
    web_ui_rt::ts_rt_set_console(Some(hook));
    game.init().unwrap();
    assert!(matches!(startup_state(), StartupState::Pending(_)));
    ts_rt_run_loop();
    web_ui_rt::ts_rt_set_console(None);
    match startup_state() {
        StartupState::Failed { module, message } => {
            assert_eq!(module, "bad.ts");
            assert_eq!(message, "Error: no network");
        }
        other => panic!("{other:?}"),
    }
    // Reported once, as an unhandled rejection that names the module (not once per importer).
    let log = LOG.with(|l| l.borrow().clone());
    let reports: Vec<_> = log
        .iter()
        .filter(|l| l.contains("Unhandled promise rejection"))
        .collect();
    assert_eq!(reports.len(), 1, "{log:?}");
    assert!(
        reports[0].starts_with("4:")
            && reports[0].contains("bad.ts")
            && reports[0].contains("no network")
    );
}

// M9-TS-3210
//
// While a module waits, the startup is pending and names the module (the host's ten-second warning
// uses it); it is done once the await finishes.
#[test]
fn a_pending_module_is_named_until_it_finishes() {
    let _serial = platypus_ts_tests::serial();
    let files = files_of("a_sibling_runs_while_the_other_is_pending");
    let game = compile_and_load(
        &bundle_of(&files).source,
        &JitOptions {
            prefix: "tm7_",
            struct_base: 917_000,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    match startup_state() {
        StartupState::Pending(modules) => assert_eq!(modules[0], "b.ts"),
        other => panic!("{other:?}"),
    }
    ts_rt_run_loop();
    assert_eq!(startup_state(), StartupState::Done);
}
