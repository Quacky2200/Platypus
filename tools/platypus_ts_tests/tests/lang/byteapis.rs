// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The byte and text APIs: `instanceof` on typed arrays, `TextEncoder` / `TextDecoder`, `atob` /
//! `btoa`, `Uint8Array.from` / `of` / base64 / hex, `Buffer` and `Blob`. The cases of
//! `bench/bytes_cases.ts` run in the normal, stress GC and collection-off modes and are compared
//! with bun (`bench/gen_cases_expected.ts bytes_cases.ts`). IDs are in TESTS.md.
use crate::cases::{case, run};
use serde_json::Value;

const CASES: &str = include_str!("../../bench/bytes_cases.ts");

fn expected(name: &str) -> String {
    let v: Value = serde_json::from_str(include_str!("../../bench/bytes_expected.json")).unwrap();
    v[name].as_str().unwrap().to_string()
}

fn bun_case(name: &str, prefix: &'static str, base: u32) {
    let got = run(case(CASES, name), prefix, base).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(got, expected(name), "{name}");
}

macro_rules! bun_tests {
    ($( $id:literal $name:ident $prefix:literal $base:literal; )*) => {$(
        #[doc = concat!("M9-TS-", stringify!($id))]
        #[test]
        fn $name() {
            bun_case(stringify!($name), $prefix, $base);
        }
    )*};
}

bun_tests! {
    3730 instanceof_on_typed_and_dynamic_values "by0_" 430000;
    3731 text_encoder_encode_and_into "by1_" 431000;
    3732 text_decoder_basics "by2_" 432000;
    3733 text_decoder_fatal_and_labels "by3_" 433000;
    3734 text_decoder_streaming "by4_" 434000;
    3735 atob_and_btoa "by5_" 435000;
    3736 typed_from_of_and_map "by6_" 436000;
    3737 uint8array_base64_and_hex "by7_" 437000;
    3738 buffer_encodings_round_trip "by8_" 438000;
    3739 buffer_read_write_every_width "by9_" 439000;
    3740 buffer_read_write_errors "by10_" 440000;
    3741 buffer_methods "by11_" 441000;
    3742 blob_parts_slice_and_reads "by12_" 442000;
    3743 blob_promises "by13_" 443000;
    3744 buffers_kept_through_other_values "by14_" 444000;
    3745 encode_decode_loop_checksum "by15_" 445000;
}
