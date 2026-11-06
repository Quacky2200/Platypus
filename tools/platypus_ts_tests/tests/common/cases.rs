// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Shared by the bun-compared language tests: a program of `//## name` cases (each defines
//! `function t(): string`) is compiled in the process, run in the normal, stress GC and
//! collection-off modes, and compared with what bun gives for the same case.
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use web_ui_ts::jit::{JitOptions, compile_and_load};

/// The source of the case `name` in `cases` (split at `//## name` lines).
pub fn case(cases: &'static str, name: &str) -> &'static str {
    let mut parts = cases.split("//## ");
    parts.next();
    for p in parts {
        let (head, body) = p.split_once('\n').unwrap();
        if head.trim() == name {
            return body;
        }
    }
    panic!("no case {name}")
}

/// Compiles `src` and runs `t()` in the three GC modes; all must agree.
pub fn run(src: &str, prefix: &'static str, struct_base: u32) -> Result<String, String> {
    run_with(src, prefix, struct_base, &[])
}

/// Like `run`, with engine API manifests (JSON) the program may call.
pub fn run_with(
    src: &str,
    prefix: &'static str,
    struct_base: u32,
    manifests: &[String],
) -> Result<String, String> {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let game = compile_and_load(
        &format!("{src}\nexport function run(): string {{ return t(); }}\n"),
        &JitOptions {
            prefix,
            struct_base,
            manifests,
            ..Default::default()
        },
    )?;
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
        game.init()?;
        let t: unsafe extern "C" fn() -> *mut u8 =
            unsafe { std::mem::transmute(game.function("run").ok_or("no function run")?) };
        let p = unsafe { t() };
        assert!(!p.is_null());
        out.push(String::from_utf16_lossy(
            &unsafe { Str::from_raw(p as *const Str) }.to_utf16(),
        ));
    }
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(true);
    if out.iter().any(|r| *r != out[0]) {
        return Err(format!("GC modes disagree: {out:?}"));
    }
    Ok(out.remove(0))
}
