// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A closure inside a variable's own initializer may name the variable
//! (`const id = watch(() => clear(id))`). IDs are in TESTS.md (M9-TS-3720..).

// M9-TS-3720: the closure sees the value the call returned, when it runs later; a number, a string
// and a class instance, each with an inferred type.
#[test]
fn a_closure_in_a_const_initializer_can_name_the_const() {
    let got = crate::cases::run(
        r#"
class Handle { n: number = 5; }
let saved: () => void = () => {};
let cleared = 0;
let log = "";
function watch(cb: () => void): number { saved = cb; return 7; }
function name(cb: () => void): string { saved = cb; return "w"; }
function make(cb: () => void): Handle { saved = cb; return new Handle(); }
function t(): string {
  const id = watch(() => { cleared += id; });
  saved();
  saved();
  const label = name(() => { log += label; });
  saved();
  const h = make(() => { cleared += h.n; });
  saved();
  return cleared + " " + log;
}
"#,
        "scc_",
        1_600_000,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(got, "19 w");
}

// M9-TS-3721: with an annotation the cell is made first too, and `let` works alike.
#[test]
fn an_annotated_or_let_variable_can_be_named_by_its_own_initializer_closure() {
    let got = crate::cases::run(
        r#"
let saved: () => void = () => {};
let total = 0;
function watch(cb: () => void): number { saved = cb; return 3; }
function t(): string {
  const a: number = watch(() => { total += a; });
  saved();
  let b = watch(() => { total += b; });
  saved();
  return String(total);
}
"#,
        "scd_",
        1_601_000,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(got, "6");
}
