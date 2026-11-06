// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Stack values: value-class objects that never leave their function become scalars
//! (`web_ui_ts::sroa`). IDs are in TESTS.md (M9-TS-720..).

fn ir_of(src: &str) -> String {
    let manifests = platypus_engine_api::platypus_math::manifests();
    web_ui_ts::compile_with_manifests(src, "speed", "sra_", 9000, &manifests)
        .unwrap_or_else(|e| panic!("{e}"))
        .ir
}

// M9-TS-720
//
// A vector that is built, combined and read in one function is never allocated.
#[test]
fn a_vector_that_stays_in_its_function_is_not_allocated() {
    let ir = ir_of(
        "export function f(a: number, b: number): number {
           const v = new Vector3(a, b, 1).add(new Vector3(1, 2, 3)).scale(2);
           return v.x + v.y + v.magnitude;
         }",
    );
    assert!(!ir.contains("new#"), "{ir}");
}

// M9-TS-721
//
// A vector that is returned, stored or passed to a script function stays an object.
#[test]
fn a_vector_that_escapes_stays_an_object() {
    let ret = ir_of("export function g(a: number): Vector3 { return new Vector3(a, 0, 0); }");
    assert!(ret.contains("new#"), "{ret}");
    let stored = ir_of(
        "class H { v: Vector3 = Vector3.zero; }
         export function s(h: H, a: number): void { const v = new Vector3(a, 0, 0); h.v = v; }",
    );
    assert!(stored.contains("new#"), "{stored}");
}

// M9-TS-722
//
// Aliases keep their meaning: changing one of two names for a vector changes both (so they stay
// one object); a copy nobody changes is scalar.
#[test]
fn aliased_vectors_that_change_stay_one_object() {
    let ir = ir_of(
        "export function a(x: number): number {
           const p = new Vector3(x, 0, 0);
           const q = p;
           q.x = 5;
           return p.x;
         }",
    );
    assert!(ir.contains("new#"), "{ir}");
}
