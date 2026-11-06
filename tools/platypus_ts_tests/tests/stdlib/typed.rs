// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/typed.ts` against bun (`bench/gen_typed_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-313..).
use platypus_ts_tests::typed_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_ty_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/typed_expected.json")).unwrap()
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

// M9-TS-313
#[test]
fn _int8_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int8Array_stores", ts_ty_Int8Array_stores());
    });
}

// M9-TS-314
#[test]
fn _int8_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int8Array_make", ts_ty_Int8Array_make());
    });
}

// M9-TS-315
#[test]
fn _int8_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int8Array_methods", ts_ty_Int8Array_methods());
    });
}

// M9-TS-316
#[test]
fn _int8_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int8Array_inplace", ts_ty_Int8Array_inplace());
    });
}

// M9-TS-317
#[test]
fn _int8_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int8Array_iterate", ts_ty_Int8Array_iterate());
    });
}

// M9-TS-318
#[test]
fn _uint8_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint8Array_stores", ts_ty_Uint8Array_stores());
    });
}

// M9-TS-319
#[test]
fn _uint8_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint8Array_make", ts_ty_Uint8Array_make());
    });
}

// M9-TS-320
#[test]
fn _uint8_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint8Array_methods", ts_ty_Uint8Array_methods());
    });
}

// M9-TS-321
#[test]
fn _uint8_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint8Array_inplace", ts_ty_Uint8Array_inplace());
    });
}

// M9-TS-322
#[test]
fn _uint8_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint8Array_iterate", ts_ty_Uint8Array_iterate());
    });
}

// M9-TS-323
#[test]
fn _uint8_clamped_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint8ClampedArray_stores", ts_ty_Uint8ClampedArray_stores());
    });
}

// M9-TS-324
#[test]
fn _uint8_clamped_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint8ClampedArray_make", ts_ty_Uint8ClampedArray_make());
    });
}

// M9-TS-325
#[test]
fn _uint8_clamped_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str(
            "Uint8ClampedArray_methods",
            ts_ty_Uint8ClampedArray_methods(),
        );
    });
}

// M9-TS-326
#[test]
fn _uint8_clamped_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str(
            "Uint8ClampedArray_inplace",
            ts_ty_Uint8ClampedArray_inplace(),
        );
    });
}

// M9-TS-327
#[test]
fn _uint8_clamped_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str(
            "Uint8ClampedArray_iterate",
            ts_ty_Uint8ClampedArray_iterate(),
        );
    });
}

// M9-TS-328
#[test]
fn _int16_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int16Array_stores", ts_ty_Int16Array_stores());
    });
}

// M9-TS-329
#[test]
fn _int16_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int16Array_make", ts_ty_Int16Array_make());
    });
}

// M9-TS-330
#[test]
fn _int16_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int16Array_methods", ts_ty_Int16Array_methods());
    });
}

// M9-TS-331
#[test]
fn _int16_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int16Array_inplace", ts_ty_Int16Array_inplace());
    });
}

// M9-TS-332
#[test]
fn _int16_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int16Array_iterate", ts_ty_Int16Array_iterate());
    });
}

// M9-TS-333
#[test]
fn _uint16_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint16Array_stores", ts_ty_Uint16Array_stores());
    });
}

// M9-TS-334
#[test]
fn _uint16_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint16Array_make", ts_ty_Uint16Array_make());
    });
}

// M9-TS-335
#[test]
fn _uint16_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint16Array_methods", ts_ty_Uint16Array_methods());
    });
}

// M9-TS-336
#[test]
fn _uint16_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint16Array_inplace", ts_ty_Uint16Array_inplace());
    });
}

// M9-TS-337
#[test]
fn _uint16_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint16Array_iterate", ts_ty_Uint16Array_iterate());
    });
}

// M9-TS-338
#[test]
fn _int32_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int32Array_stores", ts_ty_Int32Array_stores());
    });
}

// M9-TS-339
#[test]
fn _int32_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int32Array_make", ts_ty_Int32Array_make());
    });
}

// M9-TS-340
#[test]
fn _int32_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int32Array_methods", ts_ty_Int32Array_methods());
    });
}

// M9-TS-341
#[test]
fn _int32_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int32Array_inplace", ts_ty_Int32Array_inplace());
    });
}

// M9-TS-342
#[test]
fn _int32_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Int32Array_iterate", ts_ty_Int32Array_iterate());
    });
}

// M9-TS-343
#[test]
fn _uint32_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint32Array_stores", ts_ty_Uint32Array_stores());
    });
}

// M9-TS-344
#[test]
fn _uint32_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint32Array_make", ts_ty_Uint32Array_make());
    });
}

// M9-TS-345
#[test]
fn _uint32_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint32Array_methods", ts_ty_Uint32Array_methods());
    });
}

// M9-TS-346
#[test]
fn _uint32_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint32Array_inplace", ts_ty_Uint32Array_inplace());
    });
}

// M9-TS-347
#[test]
fn _uint32_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Uint32Array_iterate", ts_ty_Uint32Array_iterate());
    });
}

// M9-TS-348
#[test]
fn _float32_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float32Array_stores", ts_ty_Float32Array_stores());
    });
}

// M9-TS-349
#[test]
fn _float32_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float32Array_make", ts_ty_Float32Array_make());
    });
}

// M9-TS-350
#[test]
fn _float32_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float32Array_methods", ts_ty_Float32Array_methods());
    });
}

// M9-TS-351
#[test]
fn _float32_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float32Array_inplace", ts_ty_Float32Array_inplace());
    });
}

// M9-TS-352
#[test]
fn _float32_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float32Array_iterate", ts_ty_Float32Array_iterate());
    });
}

// M9-TS-353
#[test]
fn _float64_array_stores_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float64Array_stores", ts_ty_Float64Array_stores());
    });
}

// M9-TS-354
#[test]
fn _float64_array_make_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float64Array_make", ts_ty_Float64Array_make());
    });
}

// M9-TS-355
#[test]
fn _float64_array_methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float64Array_methods", ts_ty_Float64Array_methods());
    });
}

// M9-TS-356
#[test]
fn _float64_array_inplace_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float64Array_inplace", ts_ty_Float64Array_inplace());
    });
}

// M9-TS-357
#[test]
fn _float64_array_iterate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("Float64Array_iterate", ts_ty_Float64Array_iterate());
    });
}

// M9-TS-358
#[test]
fn set_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("setErrors", ts_ty_setErrors());
    });
}

// M9-TS-359
#[test]
fn float_sorting_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("floatSorting", ts_ty_floatSorting());
    });
}
