// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Assets.exists` / `list` / `packOf` from compiled programs (`bench/assets_native.ts`), with the
//! index the host fills, in the normal, stress GC and collection-off modes (M9-TS-2557).
use platypus_engine_api::assets;
use platypus_ts_tests::assets_native_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn index() {
    assets::reset();
    assets::set_asset_index(vec![
        ("assets/sfx/b.wav".into(), "scene-boss".into()),
        ("assets/sfx/a.wav".into(), "assets".into()),
        ("assets/sfx/c.ogg".into(), "assets".into()),
        ("assets/sfx/deep/d.wav".into(), "assets".into()),
    ]);
}

fn text(p: *mut u8) -> String {
    assert!(!p.is_null());
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

// M9-TS-2557
#[test]
fn a_compiled_program_lists_the_sorted_keys_of_a_glob() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_asn_init() };
    let want = "2|assets/sfx/a.wav,assets/sfx/b.wav|true|false|true|boss|none|empty";
    index();
    assert_eq!(text(unsafe { ts_asn_files() }), want);
    ts_rt_gc_stress(true);
    index();
    assert_eq!(text(unsafe { ts_asn_files() }), want);
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    index();
    assert_eq!(text(unsafe { ts_asn_files() }), want);
    ts_rt_gc_enable(true);
    assets::reset();
}
