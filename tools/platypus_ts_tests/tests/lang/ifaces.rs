// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Interfaces implemented by classes (and by plain objects): dispatch through the interface,
//! `extends` between interfaces, optional members, writes, `instanceof` on an interface value. The
//! programs of `bench/ifaces_cases.ts` run in the normal, stress GC and collection-off modes and
//! are compared with bun (`bench/gen_cases_expected.ts`). IDs are in TESTS.md (M9-TS-3140..).
use crate::cases::{case, run};
use serde_json::Value;

const CASES: &str = include_str!("../../bench/ifaces_cases.ts");

fn expected(name: &str) -> String {
    let v: Value = serde_json::from_str(include_str!("../../bench/ifaces_expected.json")).unwrap();
    v[name].as_str().unwrap().to_string()
}

fn bun_case(name: &str, prefix: &'static str, base: u32) {
    let got = run(case(CASES, name), prefix, base).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(got, expected(name), "{name}");
}

macro_rules! bun_tests {
    ($( $id:literal $fn:ident $name:literal $prefix:literal $base:literal; )*) => {$(
        #[doc = concat!("M9-TS-", stringify!($id))]
        #[test]
        fn $fn() {
            bun_case($name, $prefix, $base);
        }
    )*};
}

bun_tests! {
    3140 basic_dispatch "basic_dispatch" "if0_" 700000;
    3141 object_literals_and_mixed "object_literals_and_mixed" "if1_" 701000;
    3142 extends_and_subclasses "extends_and_subclasses" "if2_" 702000;
    3143 optional_members_and_instanceof "optional_members_and_instanceof" "if3_" 703000;
    3144 writes_through_the_interface "writes_through_the_interface" "if4_" 704000;
    3145 closures_and_containers "closures_and_containers" "if5_" 705000;
    3146 composite_abstract_and_this "composite_abstract_and_this" "if6_" 706000;
}

// M9-TS-3147
#[test]
fn spreading_an_interface_implemented_by_classes_is_a_compile_error() {
    let _serial = platypus_ts_tests::serial();
    let e = web_ui_ts::jit::compile_and_load(
        "interface Shape { tag: string }
class C implements Shape { tag = \"c\"; }
export function run(): string { const x: Shape = new C(); const y = { ...x, extra: 1 }; return y.tag; }",
        &web_ui_ts::jit::JitOptions {
            prefix: "if7_",
            struct_base: 707_000,
            ..Default::default()
        },
    )
    .err()
    .expect("should not compile");
    assert!(
        e.contains("interface Shape is implemented by classes"),
        "{e}"
    );
}
