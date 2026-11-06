// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/segmenter.ts` against bun (`bench/gen_segmenter_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-456..).
use platypus_ts_tests::segmenter_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_sg_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/segmenter_expected.json")).unwrap()
}

fn check_str(name: &str, got: *mut u8) {
    let want: Vec<u16> = expected()[name]["s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u16)
        .collect();
    assert!(!got.is_null(), "{name} returned null");
    let got = unsafe { Str::from_raw(got as *const Str) }.to_utf16();
    assert_eq!(
        String::from_utf16_lossy(&got),
        String::from_utf16_lossy(&want),
        "{name}"
    );
}

// M9-TS-456
#[test]
fn graphemes_ascii_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("graphemesAscii", ts_sg_graphemesAscii());
    });
}

// M9-TS-457
#[test]
fn graphemes_combining_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("graphemesCombining", ts_sg_graphemesCombining());
    });
}

// M9-TS-458
#[test]
fn graphemes_emoji_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("graphemesEmoji", ts_sg_graphemesEmoji());
    });
}

// M9-TS-459
#[test]
fn words_latin_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("wordsLatin", ts_sg_wordsLatin());
    });
}

// M9-TS-460
#[test]
fn word_like_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("wordLike", ts_sg_wordLike());
    });
}

// M9-TS-461
#[test]
fn sentences_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("sentences", ts_sg_sentences());
    });
}

// M9-TS-462
#[test]
fn containing_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("containing", ts_sg_containing());
    });
}

// M9-TS-463
#[test]
fn default_granularity_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("defaultGranularity", ts_sg_defaultGranularity());
    });
}

// M9-TS-464
#[test]
fn bad_granularity_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("badGranularity", ts_sg_badGranularity());
    });
}
