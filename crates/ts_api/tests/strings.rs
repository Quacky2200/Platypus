// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Strings across the boundary: `&str` parameters read from a script string object, `String`
//! results handed back through a per-thread buffer. IDs are in TESTS.md (M9-API-*).
use ts_api::{Ty, ts_api};

/// A script string object laid out as the compiled code makes them: a 48 byte header, then bytes.
fn script_string(bytes: &[u8], well_formed: bool) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&0xA11A_0003u64.to_le_bytes()); // header
    v.extend_from_slice(&(bytes.len() as i64).to_le_bytes()); // byte_len
    v.extend_from_slice(&0i64.to_le_bytes()); // utf16_len (unused here)
    v.extend_from_slice(&(if well_formed { 2u64 } else { 0 }).to_le_bytes()); // flags
    v.extend_from_slice(&[0u8; 16]); // cursor
    v.extend_from_slice(bytes);
    v
}

#[ts_api(namespace = "Text")]
pub mod text_api {
    /// The bytes of a string.
    pub fn byte_len(s: &str) -> u32 {
        s.len() as u32
    }

    /// Shouts.
    pub fn shout(s: &str, times: f64) -> String {
        format!("{}{}", s.to_uppercase(), "!".repeat(times as usize))
    }

    pub fn owned(s: String) -> u32 {
        s.chars().count() as u32
    }
}

// M9-API-21
#[test]
fn strings_cross_as_script_string_objects_and_a_returned_buffer() {
    let m = text_api::MANIFEST;
    let shout = m.functions.iter().find(|f| f.name == "shout").unwrap();
    assert_eq!(shout.params[0].ty, Ty::Str);
    assert_eq!(shout.ret, Ty::Str);
    assert_eq!(shout.symbol, "platypus_Text_shout");
    let d = m.to_dts();
    assert!(
        d.contains("function shout(s: string, times: number): string;"),
        "{d}"
    );
    assert!(m.to_json().contains("\"type\":\"string\""));

    let hello = script_string("héllo".as_bytes(), true);
    assert_eq!(
        unsafe { text_api::platypus_Text_byteLen(hello.as_ptr()) },
        6
    );
    assert_eq!(unsafe { text_api::platypus_Text_owned(hello.as_ptr()) }, 5);
    let mut len = 0i64;
    let p = unsafe { text_api::platypus_Text_shout(hello.as_ptr(), 3.0, &mut len) };
    let out = unsafe { std::slice::from_raw_parts(p, len as usize) };
    assert_eq!(std::str::from_utf8(out).unwrap(), "HÉLLO!!!");
}

// M9-API-22
#[test]
fn a_lone_surrogate_becomes_one_replacement_character() {
    // "a", a lone high surrogate (ED A0 80), "b".
    let s = script_string(&[b'a', 0xED, 0xA0, 0x80, b'b'], false);
    let got = unsafe { ts_api::script_str(s.as_ptr()) };
    assert_eq!(got, "a\u{FFFD}b");
    assert!(matches!(got, std::borrow::Cow::Owned(_)));
    // A well formed string is borrowed, not copied; null is empty.
    let ok = script_string(b"plain", true);
    assert!(matches!(
        unsafe { ts_api::script_str(ok.as_ptr()) },
        std::borrow::Cow::Borrowed("plain")
    ));
    assert_eq!(unsafe { ts_api::script_str(std::ptr::null()) }, "");
}
